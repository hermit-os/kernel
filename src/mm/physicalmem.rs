use core::alloc::AllocError;
use core::sync::atomic::{AtomicUsize, Ordering};
use core::{fmt, iter, ptr, slice};

use align_address::Align;
use bitvec::prelude::{BitSlice, Lsb0};
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
const WORDS_PER_BLOCK: usize = FRAMES_PER_BLOCK / u64::BITS as usize;

static PHYSICAL_FRAMES: InterruptTicketMutex<FrameBitmap<'static>> =
	InterruptTicketMutex::new(FrameBitmap::empty());
pub static TOTAL_MEMORY: AtomicUsize = AtomicUsize::new(0);

/// Bitmap of physical frames, where a set bit marks a free frame.
/// Optimized for fast finding of the next free page and the next free "Block" (HugePage)
struct FrameBitmap<'a> {
	/// Physical address of the frame at index 0, aligned to [`BLOCK_SIZE`].
	base: usize,
	/// Length is a multiple of [`WORDS_PER_BLOCK`].
	words: &'a mut [u64],
	/// Index of the lowest free frame, or `bits.len()`.
	first_free: usize,
	/// Index of the first frame of the lowest completely free [`BLOCK_SIZE`] block, or `bits.len()`.
	first_free_block: usize,
}

impl<'a> FrameBitmap<'a> {
	/// Creates a bitmap without any frames.
	const fn empty() -> Self {
		Self {
			base: 0,
			words: &mut [],
			first_free: 0,
			first_free_block: 0,
		}
	}

	/// Creates a bitmap with all frames allocated.
	///
	/// `words` must be zeroed.
	fn new(base: usize, words: &'a mut [u64]) -> Self {
		assert!(base.is_multiple_of(BLOCK_SIZE));
		assert!(words.len().is_multiple_of(WORDS_PER_BLOCK));
		let first_free = words.len() * u64::BITS as usize;
		Self {
			base,
			words,
			first_free,
			first_free_block: first_free,
		}
	}

	fn bits(&self) -> &BitSlice<u64, Lsb0> {
		BitSlice::from_slice(self.words)
	}

	fn bits_mut(&mut self) -> &mut BitSlice<u64, Lsb0> {
		BitSlice::from_slice_mut(self.words)
	}

	fn index(&self, addr: usize) -> usize {
		(addr - self.base) / FRAME_SIZE
	}

	fn addr(&self, index: usize) -> usize {
		self.base + index * FRAME_SIZE
	}

	fn is_block_free(&self, index: usize) -> bool {
		self.bits()[index..][..FRAMES_PER_BLOCK].all()
	}

	/// Find the next free entry in the bitmap
	fn next_free(&self, from: usize) -> usize {
		self.bits()[from..]
			.first_one()
			.map_or(self.bits().len(), |i| from + i)
	}

	/// Find the next free block in the bitmap.
	/// `from` must be a multiple of [`FRAMES_PER_BLOCK`].
	fn next_free_block(&self, from: usize) -> usize {
		(from..self.bits().len())
			.step_by(FRAMES_PER_BLOCK)
			.find(|index| self.is_block_free(*index))
			.unwrap_or(self.bits().len())
	}

	fn allocate(&mut self, layout: PageLayout) -> Result<PageRange, AllocError> {
		let pages = layout.size() / FRAME_SIZE;
		let mut index = if layout.size() >= BLOCK_SIZE && layout.align() >= BLOCK_SIZE {
			self.first_free_block
		} else {
			self.first_free
		};

		loop {
			index += self.bits()[index..].first_one().ok_or(AllocError)?;
			index = self.index(self.addr(index).align_up(layout.align()));
			let candidate = self.bits().get(index..index + pages).ok_or(AllocError)?;
			match candidate.last_zero() {
				Some(i) => index += i + 1,
				None => break,
			}
		}

		self.take(index, index + pages);
		Ok(PageRange::from_start_len(self.addr(index), layout.size()).unwrap())
	}

	fn allocate_at(&mut self, range: PageRange) -> Result<(), AllocError> {
		let start = range.start().checked_sub(self.base).ok_or(AllocError)? / FRAME_SIZE;
		let end = start + range.pages().get();
		if !self.bits().get(start..end).ok_or(AllocError)?.all() {
			return Err(AllocError);
		}
		self.take(start, end);
		Ok(())
	}

	fn deallocate(&mut self, range: PageRange) {
		let start = self.index(range.start());
		let end = start + range.pages().get();
		let bits = &mut self.bits_mut()[start..end];
		assert!(bits.not_any(), "{range:#x?} is not allocated");
		bits.fill(true);

		self.first_free = self.first_free.min(start);
		let mut blocks = (start.align_down(FRAMES_PER_BLOCK)..end.min(self.first_free_block))
			.step_by(FRAMES_PER_BLOCK);
		if let Some(block) = blocks.find(|index| self.is_block_free(*index)) {
			self.first_free_block = block;
		}
	}

	/// Marks the free frames `start..end` as allocated.
	fn take(&mut self, start: usize, end: usize) {
		self.bits_mut()[start..end].fill(false);

		if (start..end).contains(&self.first_free) {
			self.first_free = self.next_free(end);
		}
		if start < self.first_free_block + FRAMES_PER_BLOCK && self.first_free_block < end {
			self.first_free_block = self.next_free_block(end.align_up(FRAMES_PER_BLOCK));
		}
	}

	fn free_ranges(&self) -> impl Iterator<Item = PageRange> + '_ {
		let mut index = self.first_free;
		iter::from_fn(move || {
			let start = index + self.bits()[index..].first_one()?;
			let len = self.bits()[start..]
				.first_zero()
				.unwrap_or(self.bits().len() - start);
			index = start + len;
			Some(PageRange::new(self.addr(start), self.addr(index)).unwrap())
		})
	}
}

pub struct FrameAlloc;

impl PageRangeAllocator for FrameAlloc {
	unsafe fn init() {
		unsafe {
			init();
		}
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

pub fn total_memory_size() -> usize {
	TOTAL_MEMORY.load(Ordering::Relaxed)
}

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

unsafe fn detect_from_start_info() {
	for reservation in reservations() {
		debug!("Memory reservation: {reservation:#x?}");
	}

	let base = free_ranges()
		.map(PageRange::start)
		.min()
		.unwrap()
		.align_down(BLOCK_SIZE);
	let end = free_ranges().map(PageRange::end).max().unwrap();
	let word_count = (end - base).div_ceil(BLOCK_SIZE) * WORDS_PER_BLOCK;
	let bitmap_size = (word_count * size_of::<u64>()).align_up(FRAME_SIZE);

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
	let words = unsafe {
		let words = ptr::with_exposed_provenance_mut::<u64>(bitmap_range.start());
		words.write_bytes(0, word_count);
		slice::from_raw_parts_mut(words, word_count)
	};
	let mut bitmap = FrameBitmap::new(base, words);
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

	let free_frames = PHYSICAL_FRAMES.lock().bits().count_ones();
	TOTAL_MEMORY.store(free_frames * FRAME_SIZE, Ordering::Relaxed);
}

unsafe fn init() {
	if cfg!(target_arch = "x86_64") && DeviceAlloc.phys_offset() != VirtAddr::zero() {
		let start = DeviceAlloc.phys_offset();
		let count = DeviceAlloc.phys_offset().as_u64() / HugePageSize::SIZE;
		let count = usize::try_from(count).unwrap();
		paging::unmap::<HugePageSize>(start, count);
	}

	unsafe {
		detect_from_start_info();
	}
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
			assert_eq!(self.first_free, self.next_free(0));
			assert_eq!(self.first_free_block, self.next_free_block(0));
		}
	}

	#[test]
	fn allocate() {
		let mut words = [0; WORDS_PER_BLOCK];
		let mut bitmap = FrameBitmap::new(BASE, &mut words);
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
		let mut bitmap = FrameBitmap::new(BASE, &mut words);
		let frame_count = bitmap.bits().len();
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
		let mut bitmap = FrameBitmap::new(BASE, &mut words);
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
		let mut bitmap = FrameBitmap::new(BASE, &mut words);
		bitmap.deallocate(relative_range(0x1000, 0x4000));
		assert!(bitmap.allocate_at(relative_range(0x2000, 0x5000)).is_err());
		assert!(bitmap.allocate_at(relative_range(0x2000, 0x3000)).is_ok());
		assert!(bitmap.allocate_at(relative_range(0x2000, 0x3000)).is_err());
		bitmap.assert_first_free_correct();

		let outside = PageRange::new(BASE - FRAME_SIZE, BASE).unwrap();
		assert!(bitmap.allocate_at(outside).is_err());
	}
}
