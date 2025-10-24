use core::alloc::AllocError;
use core::marker::PhantomData;
use core::mem::ManuallyDrop;
use core::ops::Deref;

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

pub struct PageRangeBox<A: PageRangeAllocator>(PageRange, PhantomData<A>);

impl<A: PageRangeAllocator> PageRangeBox<A> {
	pub fn new(layout: PageLayout) -> Result<Self, AllocError> {
		let range = A::allocate(layout)?;
		Ok(Self(range, PhantomData))
	}

	pub unsafe fn from_raw(range: PageRange) -> Self {
		Self(range, PhantomData)
	}

	pub fn into_raw(b: Self) -> PageRange {
		let b = ManuallyDrop::new(b);
		**b
	}
}

impl<A: PageRangeAllocator> Drop for PageRangeBox<A> {
	fn drop(&mut self) {
		unsafe {
			A::deallocate(self.0);
		}
	}
}

impl<A: PageRangeAllocator> Deref for PageRangeBox<A> {
	type Target = PageRange;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}
