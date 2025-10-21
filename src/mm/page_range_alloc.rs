use core::alloc::AllocError;

use free_list::{PageLayout, PageRange};

/// An allocator that allocates memory in page granularity.
pub trait PageRangeAllocator {
	unsafe fn init();

	/// Attempts to allocate a range of memory in page granularity.
	fn allocate(layout: PageLayout) -> Result<PageRange, AllocError>;

	/// Attempts to allocate the pages described by `range`.
	fn allocate_at(range: PageRange) -> Result<(), AllocError>;

	/// Deallocates the pages described by `range`.
	///
	/// # Safety
	///
	/// - `range` must described a range of pages _currently allocated_ via this allocator.
	unsafe fn deallocate(range: PageRange);
}
