use core::alloc::AllocError;
use core::ops::Range;
use core::{fmt, iter, ptr, slice};

use align_address::Align;
use free_list::{PageLayout, PageRange};
use hermit_sync::InterruptTicketMutex;
#[cfg(feature = "hermit-entry")]
use memory_addresses::PhysAddr;
use memory_addresses::VirtAddr;

#[cfg(all(target_arch = "x86_64", feature = "hermit-entry"))]
use crate::arch::mm::paging::PageTableEntryFlagsExt;
use crate::arch::mm::paging::{self, BasePageSize, HugePageSize, LargePageSize, PageSize};
use crate::env::{self, MemmapType, StartInfo};
use crate::mm::device_alloc::DeviceAlloc;
use crate::mm::{PageRangeAllocator, PageRangeBox};

/// A page is the granularity of the bitmap
const FRAME_SIZE: usize = BasePageSize::SIZE as usize;
/// A block is a common size of a page range whose search we speed up
const BLOCK_SIZE: usize = LargePageSize::SIZE as usize;
const FRAMES_PER_BLOCK: usize = BLOCK_SIZE / FRAME_SIZE;
const FULL_BLOCK: u16 = FRAMES_PER_BLOCK as u16;
const WORDS_PER_BLOCK: usize = FRAMES_PER_BLOCK / BITS;
const BITS: usize = u64::BITS as usize;

static PHYSICAL_FRAMES: InterruptTicketMutex<FrameBitmap<'static>> =
	InterruptTicketMutex::new(FrameBitmap::empty());

/// Bitmap of physical frames, where a set bit marks a free frame.
/// Optimized for fast finding of the next free page and the next free "Block" (HugePage)
struct FrameBitmap<'a> {
	/// Physical address of the frame at index 0, aligned to [`BLOCK_SIZE`].
	base: usize,
	/// Length is a multiple of [`WORDS_PER_BLOCK`].
	words: &'a mut [u64],
	/// Number of free frames per block.
	free_counts: &'a mut [u16],
	/// Index of the lowest free frame, or `len()`.
	first_free: usize,
	/// Index of the first frame of the lowest completely free [`BLOCK_SIZE`] block, or `len()`.
	first_free_block: usize,
}

/// Splits the non-empty range of `bits` into the words it touches, yielding `(word index, mask)`
/// where `mask` selects the range's bits within that word.
///
/// Example: `into_bitmasks(60..132)` yields
/// `(0, 0xF000_0000_0000_0000)`, `(1, 0xFFFF_FFFF_FFFF_FFFF)`, `(2, 0x0000_0000_0000_000F)`.
fn into_bitmasks(bits: Range<usize>) -> impl DoubleEndedIterator<Item = (usize, u64)> {
	(bits.start / BITS..bits.end.div_ceil(BITS)).map(move |w| {
		let lo = bits.start.max(w * BITS) - w * BITS;
		let hi = bits.end.min(w * BITS + BITS) - w * BITS;
		(w, (!0 >> (BITS - (hi - lo))) << lo)
	})
}

/// Returns the block range if `frames` consists of whole blocks.
fn into_block_range(frames: Range<usize>) -> Option<Range<usize>> {
	(frames.start.is_multiple_of(FRAMES_PER_BLOCK) && frames.end.is_multiple_of(FRAMES_PER_BLOCK))
		.then_some(frames.start / FRAMES_PER_BLOCK..frames.end / FRAMES_PER_BLOCK)
}

impl<'a> FrameBitmap<'a> {
	/// Creates a bitmap without any frames.
	const fn empty() -> Self {
		Self {
			base: 0,
			words: &mut [],
			free_counts: &mut [],
			first_free: 0,
			first_free_block: 0,
		}
	}

	/// Creates a bitmap with all frames allocated.
	///
	/// `words` and `free_counts` must be zeroed, with one entry in `free_counts` per
	/// [`WORDS_PER_BLOCK`] words.
	fn new(base: usize, words: &'a mut [u64], free_counts: &'a mut [u16]) -> Self {
		assert!(base.is_multiple_of(BLOCK_SIZE));
		assert!(words.len().is_multiple_of(WORDS_PER_BLOCK));
		assert_eq!(free_counts.len(), words.len() / WORDS_PER_BLOCK);
		let first_free = words.len() * BITS;
		Self {
			base,
			words,
			free_counts,
			first_free,
			first_free_block: first_free,
		}
	}

	/// Number of frames in the bitmap.
	fn len(&self) -> usize {
		self.words.len() * BITS
	}

	fn index(&self, addr: usize) -> usize {
		(addr - self.base) / FRAME_SIZE
	}

	fn addr(&self, index: usize) -> usize {
		self.base + index * FRAME_SIZE
	}

	fn is_all_free(&self, frames: Range<usize>) -> bool {
		into_bitmasks(frames).all(|(w, m)| self.words[w] & m == m)
	}

	fn is_all_allocated(&self, frames: Range<usize>) -> bool {
		if frames.len() >= FRAMES_PER_BLOCK
			&& let Some(blocks) = into_block_range(frames.clone())
		{
			return self.free_counts[blocks].iter().all(|&c| c == 0);
		}
		into_bitmasks(frames).all(|(w, m)| self.words[w] & m == 0)
	}

	/// Index of the allocated frame closest to `frames.end` or `None` if all frames in `frames` are
	/// free.
	///
	/// Example: with frames `[free, alloc, free, alloc, free]`, `rfind_allocated(0..5)` is `Some(3)`.
	fn rfind_allocated(&self, frames: Range<usize>) -> Option<usize> {
		into_bitmasks(frames).rev().find_map(|(w, m)| {
			let allocated = !self.words[w] & m;
			(allocated != 0).then(|| w * BITS + BITS - 1 - allocated.leading_zeros() as usize)
		})
	}

	fn is_block_free(&self, index: usize) -> bool {
		self.free_counts[index / FRAMES_PER_BLOCK] == FULL_BLOCK
	}

	/// Index of the first free (`free == true`) or allocated frame at or after `from`.
	fn find(&self, from: usize, free: bool) -> Option<usize> {
		let flip = if free { 0 } else { !0 };
		let w = from / BITS;
		let first = (*self.words.get(w)? ^ flip) & (!0 << (from % BITS));
		if first != 0 {
			return Some(w * BITS + first.trailing_zeros() as usize);
		}
		self.words[w + 1..]
			.iter()
			.position(|&x| x ^ flip != 0)
			.map(|i| (w + 1 + i) * BITS + (self.words[w + 1 + i] ^ flip).trailing_zeros() as usize)
	}

	/// Find the next free entry in the bitmap
	fn next_free(&self, from: usize) -> usize {
		self.find(from, true).unwrap_or(self.len())
	}

	/// Find the next free block in the bitmap.
	/// `from` must be a multiple of [`FRAMES_PER_BLOCK`].
	fn next_free_block(&self, from: usize) -> usize {
		let block = from / FRAMES_PER_BLOCK;
		self.free_counts[block..]
			.iter()
			.position(|&c| c == FULL_BLOCK)
			.map_or(self.len(), |i| (block + i) * FRAMES_PER_BLOCK)
	}

	fn allocate(&mut self, layout: PageLayout) -> Result<PageRange, AllocError> {
		let pages = layout.size() / FRAME_SIZE;
		let mut index = if layout.size() >= BLOCK_SIZE && layout.align() >= BLOCK_SIZE {
			self.first_free_block
		} else {
			self.first_free
		};
		let block_layout = layout.align() >= BLOCK_SIZE && pages.is_multiple_of(FRAMES_PER_BLOCK);

		loop {
			index = self.find(index, true).ok_or(AllocError)?;
			index = self.index(self.addr(index).align_up(layout.align()));
			if index + pages > self.len() {
				return Err(AllocError);
			}
			let allocated = if block_layout {
				// Any candidate overlapping a partially allocated block starts after that block.
				let blocks = index / FRAMES_PER_BLOCK..(index + pages) / FRAMES_PER_BLOCK;
				self.free_counts[blocks.clone()]
					.iter()
					.rposition(|&c| c != FULL_BLOCK)
					.map(|i| (blocks.start + i + 1) * FRAMES_PER_BLOCK - 1)
			} else {
				self.rfind_allocated(index..index + pages)
			};
			match allocated {
				Some(i) => index = i + 1,
				None => break,
			}
		}

		self.take(index..index + pages);
		Ok(PageRange::from_start_len(self.addr(index), layout.size()).unwrap())
	}

	fn allocate_at(&mut self, range: PageRange) -> Result<(), AllocError> {
		let start = range.start().checked_sub(self.base).ok_or(AllocError)? / FRAME_SIZE;
		let frames = start..start + range.pages().get();
		if frames.end > self.len() || !self.is_all_free(frames.clone()) {
			return Err(AllocError);
		}
		self.take(frames);
		Ok(())
	}

	fn deallocate(&mut self, range: PageRange) {
		let start = self.index(range.start());
		let frames = start..start + range.pages().get();
		assert!(
			self.is_all_allocated(frames.clone()),
			"{range:#x?} is not allocated"
		);
		for (w, m) in into_bitmasks(frames.clone()) {
			self.words[w] |= m;
		}
		self.update_counts(frames.clone(), true);

		self.first_free = self.first_free.min(frames.start);
		let mut blocks = (frames.start.align_down(FRAMES_PER_BLOCK)
			..frames.end.min(self.first_free_block))
			.step_by(FRAMES_PER_BLOCK);
		if let Some(block) = blocks.find(|index| self.is_block_free(*index)) {
			self.first_free_block = block;
		}
	}

	/// Marks the free `frames` as allocated.
	fn take(&mut self, frames: Range<usize>) {
		for (w, m) in into_bitmasks(frames.clone()) {
			self.words[w] &= !m;
		}
		self.update_counts(frames.clone(), false);

		if frames.contains(&self.first_free) {
			self.first_free = self.next_free(frames.end);
		}
		if frames.start < self.first_free_block + FRAMES_PER_BLOCK
			&& self.first_free_block < frames.end
		{
			self.first_free_block = self.next_free_block(frames.end.align_up(FRAMES_PER_BLOCK));
		}
	}

	/// Adjusts the free counters for the non-empty `frames` becoming free (`free == true`)
	/// or allocated.
	fn update_counts(&mut self, frames: Range<usize>, free: bool) {
		let first = frames.start / FRAMES_PER_BLOCK;
		let last = (frames.end - 1) / FRAMES_PER_BLOCK;
		if first == last {
			let n = frames.len() as u16;
			if free {
				self.free_counts[first] += n;
			} else {
				self.free_counts[first] -= n;
			}
			return;
		}

		let head = ((first + 1) * FRAMES_PER_BLOCK - frames.start) as u16;
		let tail = (frames.end - last * FRAMES_PER_BLOCK) as u16;
		if free {
			self.free_counts[first] += head;
			self.free_counts[last] += tail;
			self.free_counts[first + 1..last].fill(FULL_BLOCK);
		} else {
			self.free_counts[first] -= head;
			self.free_counts[last] -= tail;
			self.free_counts[first + 1..last].fill(0);
		}
	}

	fn free_frames(&self) -> usize {
		self.free_counts.iter().map(|&c| usize::from(c)).sum()
	}

	fn free_ranges(&self) -> impl Iterator<Item = PageRange> + '_ {
		let mut index = self.first_free;
		iter::from_fn(move || {
			let start = self.find(index, true)?;
			index = self.find(start, false).unwrap_or(self.len());
			Some(PageRange::new(self.addr(start), self.addr(index)).unwrap())
		})
	}
}

pub struct FrameAlloc;

impl PageRangeAllocator for FrameAlloc {
	unsafe fn init() -> usize {
		if cfg!(target_arch = "x86_64") && DeviceAlloc.phys_offset() != VirtAddr::zero() {
			let start = DeviceAlloc.phys_offset();
			let count = DeviceAlloc.phys_offset().as_u64() / HugePageSize::SIZE;
			let count = usize::try_from(count).unwrap();
			paging::unmap::<HugePageSize>(start, count);
		}

		unsafe { detect_from_start_info() }
	}

	fn allocate(layout: PageLayout) -> Result<PageRange, AllocError> {
		PHYSICAL_FRAMES.lock().allocate(layout)
	}

	fn allocate_at(range: PageRange) -> Result<(), AllocError> {
		PHYSICAL_FRAMES.lock().allocate_at(range)
	}

	unsafe fn deallocate(range: PageRange) {
		PHYSICAL_FRAMES.lock().deallocate(range);
	}
}

impl fmt::Display for FrameAlloc {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let bitmap = PHYSICAL_FRAMES.lock();
		write!(f, "FrameAlloc free ranges:")?;
		for range in bitmap.free_ranges() {
			write!(f, "\n{range:#}")?;
		}
		Ok(())
	}
}

pub type FrameBox = PageRangeBox<FrameAlloc>;

#[cfg(feature = "hermit-entry")]
pub unsafe fn map_frame_range(frame_range: PageRange) {
	use crate::arch::mm::paging::PageTableEntryFlags;

	cfg_select! {
		target_arch = "aarch64" => {
			type IdentityPageSize = BasePageSize;
		}
		target_arch = "riscv64" => {
			type IdentityPageSize = HugePageSize;
		}
		target_arch = "x86_64" => {
			type IdentityPageSize = LargePageSize;
		}
	}

	let start = frame_range
		.start()
		.align_down(IdentityPageSize::SIZE.try_into().unwrap());
	let end = frame_range
		.end()
		.align_up(IdentityPageSize::SIZE.try_into().unwrap());

	(start..end)
		.step_by(IdentityPageSize::SIZE.try_into().unwrap())
		.map(|addr| PhysAddr::new(addr.try_into().unwrap()))
		.for_each(paging::identity_map::<IdentityPageSize>);

	// Map the physical memory again if DeviceAlloc operates at an offset
	if DeviceAlloc.phys_offset() != VirtAddr::zero() {
		let flags = PageTableEntryFlags::normal();
		(start..end)
			.step_by(IdentityPageSize::SIZE.try_into().unwrap())
			.for_each(|addr| {
				let phys_addr = PhysAddr::new(addr.try_into().unwrap());
				let virt_addr = VirtAddr::from_ptr(DeviceAlloc.ptr_from::<()>(phys_addr));
				paging::map::<IdentityPageSize>(virt_addr, phys_addr, 1, flags);
			});
	}
}

/// Returns the RAM ranges from the memory map that the kernel may claim.
fn usable_ram() -> impl Iterator<Item = PageRange> {
	env::start_info()
		.memmap()
		.filter(|memmap_entry| memmap_entry.ty == MemmapType::Ram)
		.filter_map(|memmap_entry| {
			let mut start_addr = memmap_entry.phys_addr;
			let mut end_addr = start_addr + memmap_entry.len;

			// Do not free any address in the real mode addressable range.
			//
			// When claiming physical memory, ignore all addresses below this one. This ensures we
			// don't accidentally clash with hardcoded low addresses, such as `SMP_BOOT_CODE_ADDRESS`
			// in x86_64 with SMP enabled. We use a 2MIB size for now, but this is arbitrary, and could
			// likely be lowered.
			start_addr = start_addr.max(LargePageSize::SIZE as usize);

			#[cfg(all(target_arch = "x86_64", feature = "hermit-entry"))]
			if paging::is_recursive() {
				start_addr = start_addr.max(elf_symbols::executable_end().addr());
			}

			if cfg!(target_arch = "aarch64") || cfg!(target_arch = "riscv64") {
				start_addr = start_addr.max(elf_symbols::executable_end().addr());
			}

			start_addr = start_addr.align_up(0x1000);
			end_addr = end_addr.align_down(0x1000);

			PageRange::new(start_addr, end_addr).ok()
		})
}

fn reservations() -> impl Iterator<Item = PageRange> {
	let kernel_start = elf_symbols::executable_start().addr();
	let kernel_end = elf_symbols::executable_end().addr();
	let kernel_region = PageRange::containing(kernel_start, kernel_end).unwrap();

	let modules = env::start_info()
		.modules()
		.map(|module| module.phys_frame_range());

	#[cfg(feature = "hermit-entry")]
	let fdt = {
		use crate::env::FdtStartInfo;

		let fdt = env::start_info().fdt().unwrap();

		let fdt_start = env::start_info().fdt_addr().unwrap().get();
		let fdt_end = fdt_start + fdt.total_size();
		let fdt_region = PageRange::containing(fdt_start, fdt_end).unwrap();

		fdt.memory_reservations()
			.map(|reservation| {
				let start = reservation.address().addr();
				let end = start + reservation.size();
				PageRange::new(start, end).unwrap()
			})
			.chain(iter::once(fdt_region))
	};
	#[cfg(not(feature = "hermit-entry"))]
	let fdt = iter::empty();

	iter::once(kernel_region).chain(modules).chain(fdt)
}

/// Returns [`usable_ram`] without [`reservations`].
fn free_ranges() -> impl Iterator<Item = PageRange> {
	usable_ram().flat_map(|ram| {
		let mut start = ram.start();
		iter::from_fn(move || {
			while let Ok(rest) = PageRange::new(start, ram.end()) {
				let Some(reservation) = reservations()
					.filter_map(|reservation| reservation.and(rest))
					.min_by_key(|reservation| reservation.start())
				else {
					start = ram.end();
					return Some(rest);
				};
				let free = PageRange::new(start, reservation.start());
				start = reservation.end();
				if let Ok(free) = free {
					return Some(free);
				}
			}
			None
		})
	})
}

/// returns the available memory (excluding the bitmap) in Bytes.
unsafe fn detect_from_start_info() -> usize {
	for reservation in reservations() {
		debug!("Memory reservation: {reservation:#x?}");
	}

	let base = free_ranges()
		.map(PageRange::start)
		.min()
		.unwrap()
		.align_down(BLOCK_SIZE);
	let end = free_ranges().map(PageRange::end).max().unwrap();
	let block_count = (end - base).div_ceil(BLOCK_SIZE);
	let word_count = block_count * WORDS_PER_BLOCK;
	let bitmap_size =
		(word_count * size_of::<u64>() + block_count * size_of::<u16>()).align_up(FRAME_SIZE);

	// The bitmap is placed in the lowest free memory.
	// We assume this memory is already mapped, or that no new pagetables are
	// required during the setup.
	let bitmap_range = free_ranges()
		.filter_map(|range| {
			PageRange::from_start_len(range.start(), bitmap_size)
				.ok()
				.filter(|fit| range.contains(*fit))
		})
		.min_by_key(|fit| fit.start())
		.unwrap();
	debug!("Frame bitmap: {bitmap_range:#x?}");

	// We have to assume working identity mapping here, as the `DeviceAlloc` mapping is
	// only established by `map_frame_range` below, which requires the bitmap.
	let (words, free_counts) = unsafe {
		let words = ptr::with_exposed_provenance_mut::<u64>(bitmap_range.start());
		words.write_bytes(0, word_count);
		let free_counts = words.add(word_count).cast::<u16>();
		free_counts.write_bytes(0, block_count);
		(
			slice::from_raw_parts_mut(words, word_count),
			slice::from_raw_parts_mut(free_counts, block_count),
		)
	};
	let mut bitmap = FrameBitmap::new(base, words, free_counts);
	for frame_range in free_ranges() {
		bitmap.deallocate(frame_range);
	}
	bitmap.allocate_at(bitmap_range).unwrap();
	*PHYSICAL_FRAMES.lock() = bitmap;

	for frame_range in free_ranges() {
		#[cfg(feature = "hermit-entry")]
		unsafe {
			map_frame_range(frame_range);
		}
		debug!("Claimed physical memory: {frame_range:#x?}");
	}

	let free_frames = PHYSICAL_FRAMES.lock().free_frames();
	free_frames * FRAME_SIZE
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
	use super::*;

	const BASE: usize = 0x4000_0000;

	fn relative_range(start: usize, end: usize) -> PageRange {
		PageRange::new(BASE + start, BASE + end).unwrap()
	}

	fn layout(size: usize, align: usize) -> PageLayout {
		PageLayout::from_size_align(size, align).unwrap()
	}

	impl FrameBitmap<'_> {
		fn assert_first_free_correct(&self) {
			for (block, &count) in self.free_counts.iter().enumerate() {
				let words = &self.words[block * WORDS_PER_BLOCK..][..WORDS_PER_BLOCK];
				let ones: u32 = words.iter().map(|w| w.count_ones()).sum();
				assert_eq!(u32::from(count), ones);
			}
			assert_eq!(self.first_free, self.next_free(0));
			assert_eq!(self.first_free_block, self.next_free_block(0));
		}
	}

	#[test]
	fn allocate() {
		let mut words = [0; WORDS_PER_BLOCK];
		let mut free_counts = [0; 1];
		let mut bitmap = FrameBitmap::new(BASE, &mut words, &mut free_counts);
		bitmap.deallocate(relative_range(0x1000, 0x3000));
		bitmap.deallocate(relative_range(0x4000, 0x8000));

		// Skips the gap that is too small.
		let three_pages = layout(3 * FRAME_SIZE, FRAME_SIZE);
		assert_eq!(
			bitmap.allocate(three_pages),
			Ok(relative_range(0x4000, 0x7000))
		);
		assert_eq!(bitmap.allocate(three_pages), Err(AllocError));

		// Skips the frame that is not aligned.
		assert_eq!(
			bitmap.allocate(layout(FRAME_SIZE, 0x2000)),
			Ok(relative_range(0x2000, 0x3000))
		);

		let page = layout(FRAME_SIZE, FRAME_SIZE);
		assert_eq!(bitmap.allocate(page), Ok(relative_range(0x1000, 0x2000)));
		bitmap.assert_first_free_correct();

		bitmap.deallocate(relative_range(0x1000, 0x2000));
		bitmap.assert_first_free_correct();
		assert_eq!(bitmap.allocate(page), Ok(relative_range(0x1000, 0x2000)));
	}

	#[test]
	fn first_free_block() {
		let mut words = [0; 4 * WORDS_PER_BLOCK];
		let mut free_counts = [0; 4];
		let mut bitmap = FrameBitmap::new(BASE, &mut words, &mut free_counts);
		let frame_count = bitmap.len();
		assert_eq!(bitmap.first_free, frame_count);
		assert_eq!(bitmap.first_free_block, frame_count);

		bitmap.deallocate(relative_range(FRAME_SIZE, 4 * BLOCK_SIZE));
		// First page is still occupied, thus the second block is the "first_free_block"
		assert_eq!(bitmap.first_free, 1);
		assert_eq!(bitmap.first_free_block, FRAMES_PER_BLOCK);

		let block = layout(BLOCK_SIZE, BLOCK_SIZE);
		assert_eq!(
			bitmap.allocate(block),
			Ok(relative_range(BLOCK_SIZE, 2 * BLOCK_SIZE))
		);
		bitmap.assert_first_free_correct();

		// A single page taken from the first free block moves `first_free_block` on.
		let page = relative_range(2 * BLOCK_SIZE, 2 * BLOCK_SIZE + FRAME_SIZE);
		assert!(bitmap.allocate_at(page).is_ok());
		bitmap.assert_first_free_correct();
		assert_eq!(
			bitmap.allocate(block),
			Ok(relative_range(3 * BLOCK_SIZE, 4 * BLOCK_SIZE))
		);
		assert_eq!(bitmap.allocate(block), Err(AllocError));

		// Freeing the page completes the block again.
		bitmap.deallocate(page);
		bitmap.assert_first_free_correct();
		assert_eq!(bitmap.first_free_block, 2 * FRAMES_PER_BLOCK);
	}

	#[test]
	fn first_free_block_at_block_boundary() {
		let mut words = [0; 3 * WORDS_PER_BLOCK];
		let mut free_counts = [0; 3];
		let mut bitmap = FrameBitmap::new(BASE, &mut words, &mut free_counts);
		bitmap.deallocate(relative_range(0, 3 * BLOCK_SIZE));
		assert_eq!(bitmap.first_free_block, 0);

		let last_page_of_block_0 = relative_range(BLOCK_SIZE - FRAME_SIZE, BLOCK_SIZE);
		let boundary_pages = relative_range(BLOCK_SIZE - FRAME_SIZE, BLOCK_SIZE + FRAME_SIZE);

		assert!(bitmap.allocate_at(boundary_pages).is_ok());
		bitmap.assert_first_free_correct();
		assert_eq!(bitmap.first_free_block, 2 * FRAMES_PER_BLOCK);

		bitmap.deallocate(boundary_pages);
		bitmap.assert_first_free_correct();
		assert_eq!(bitmap.first_free_block, 0);

		assert!(bitmap.allocate_at(last_page_of_block_0).is_ok());
		bitmap.assert_first_free_correct();
		assert_eq!(bitmap.first_free_block, FRAMES_PER_BLOCK);

		bitmap.deallocate(last_page_of_block_0);
		bitmap.assert_first_free_correct();
		assert_eq!(bitmap.first_free_block, 0);
	}

	#[test]
	fn allocate_at() {
		let mut words = [0; WORDS_PER_BLOCK];
		let mut free_counts = [0; 1];
		let mut bitmap = FrameBitmap::new(BASE, &mut words, &mut free_counts);
		bitmap.deallocate(relative_range(0x1000, 0x4000));
		assert!(bitmap.allocate_at(relative_range(0x2000, 0x5000)).is_err());
		assert!(bitmap.allocate_at(relative_range(0x2000, 0x3000)).is_ok());
		assert!(bitmap.allocate_at(relative_range(0x2000, 0x3000)).is_err());
		bitmap.assert_first_free_correct();

		let outside = PageRange::new(BASE - FRAME_SIZE, BASE).unwrap();
		assert!(bitmap.allocate_at(outside).is_err());
	}
}
