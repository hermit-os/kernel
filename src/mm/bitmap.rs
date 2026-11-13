use core::alloc::AllocError;
use core::iter;
use core::ops::Range;

use align_address::Align;
use free_list::{PageLayout, PageRange};
use zerocopy::{FromBytes, IntoBytes};

use crate::arch::mm::paging::{LargePageSize, PageSize};
use crate::mm::physicalmem::FRAME_SIZE;

/// A block is a common size of a page range whose search we speed up
const BLOCK_SIZE: usize = LargePageSize::SIZE as usize;
const FRAMES_PER_BLOCK: usize = BLOCK_SIZE / FRAME_SIZE;
const FULL_BLOCK: u16 = FRAMES_PER_BLOCK as u16;
const WORDS_PER_BLOCK: usize = FRAMES_PER_BLOCK / BITS;
const BITS: usize = u64::BITS as usize;

/// Bitmap of physical frames, where a set bit marks a free frame.
/// Optimized for fast finding of the next free page and the next free "Block" (HugePage)
pub struct FrameBitmap<'a> {
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
	pub const fn empty() -> Self {
		Self {
			base: 0,
			words: &mut [],
			free_counts: &mut [],
			first_free: 0,
			first_free_block: 0,
		}
	}

	/// Number of `u64`s of storage needed to manage `frames`.
	pub fn required_words(frames: PageRange) -> usize {
		let (_, block_count) = Self::geometry(frames);
		block_count * WORDS_PER_BLOCK + (block_count * size_of::<u16>()).div_ceil(size_of::<u64>())
	}

	/// Creates a bitmap managing `frames` with all frames allocated.
	///
	/// `storage` holds the bitmap's data and must be at least [`Self::required_words`] long.
	pub fn new(frames: PageRange, storage: &'a mut [u64]) -> Self {
		let (base, block_count) = Self::geometry(frames);
		let word_count = block_count * WORDS_PER_BLOCK;
		storage.fill(0);
		let (words, rest) = storage.split_at_mut(word_count);
		let free_counts = <[u16]>::mut_from_bytes(rest.as_mut_bytes()).unwrap();
		Self::from_parts(base, words, &mut free_counts[..block_count])
	}

	/// Creates a bitmap with all frames allocated.
	///
	/// `words` and `free_counts` must be zeroed, with one entry in `free_counts` per
	/// [`WORDS_PER_BLOCK`] words.
	fn from_parts(base: usize, words: &'a mut [u64], free_counts: &'a mut [u16]) -> Self {
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

	/// Returns the base address and number of blocks of a bitmap managing `frames`.
	fn geometry(frames: PageRange) -> (usize, usize) {
		let base = frames.start().align_down(BLOCK_SIZE);
		(base, (frames.end() - base).div_ceil(BLOCK_SIZE))
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

	pub fn allocate(&mut self, layout: PageLayout) -> Result<PageRange, AllocError> {
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

	pub fn allocate_at(&mut self, range: PageRange) -> Result<(), AllocError> {
		let start = range.start().checked_sub(self.base).ok_or(AllocError)? / FRAME_SIZE;
		let frames = start..start + range.pages().get();
		if frames.end > self.len() || !self.is_all_free(frames.clone()) {
			return Err(AllocError);
		}
		self.take(frames);
		Ok(())
	}

	pub fn deallocate(&mut self, range: PageRange) {
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

	pub fn free_frames(&self) -> usize {
		self.free_counts.iter().map(|&c| usize::from(c)).sum()
	}

	pub fn free_ranges(&self) -> impl Iterator<Item = PageRange> + '_ {
		let mut index = self.first_free;
		iter::from_fn(move || {
			let start = self.find(index, true)?;
			index = self.find(start, false).unwrap_or(self.len());
			Some(PageRange::new(self.addr(start), self.addr(index)).unwrap())
		})
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

	/// The frames of `blocks` blocks starting at `BASE` and storage for a bitmap managing them.
	fn frames_and_storage(blocks: usize) -> (PageRange, Vec<u64>) {
		let frames = PageRange::from_start_len(BASE, blocks * BLOCK_SIZE).unwrap();
		(frames, alloc::vec![0; FrameBitmap::required_words(frames)])
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
		let (frames, mut storage) = frames_and_storage(1);
		let mut bitmap = FrameBitmap::new(frames, &mut storage);
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
		let (frames, mut storage) = frames_and_storage(4);
		let mut bitmap = FrameBitmap::new(frames, &mut storage);
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
		let (frames, mut storage) = frames_and_storage(3);
		let mut bitmap = FrameBitmap::new(frames, &mut storage);
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
		let (frames, mut storage) = frames_and_storage(1);
		let mut bitmap = FrameBitmap::new(frames, &mut storage);
		bitmap.deallocate(relative_range(0x1000, 0x4000));
		assert!(bitmap.allocate_at(relative_range(0x2000, 0x5000)).is_err());
		assert!(bitmap.allocate_at(relative_range(0x2000, 0x3000)).is_ok());
		assert!(bitmap.allocate_at(relative_range(0x2000, 0x3000)).is_err());
		bitmap.assert_first_free_correct();

		let outside = PageRange::new(BASE - FRAME_SIZE, BASE).unwrap();
		assert!(bitmap.allocate_at(outside).is_err());
	}
}
