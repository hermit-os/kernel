use core::alloc::Layout;
use core::ptr::{self, NonNull};

use align_address::Align;
use memory_addresses::VirtAddr;
use talc::base::Talc;
use talc::base::binning::Binning;
use talc::source::Source;

use super::ALLOCATOR;
use crate::arch::kernel::processor;
use crate::arch::mm::paging::{self, BasePageSize, LargePageSize, PageSize};
use crate::console;

const GROW_STEP: usize = LargePageSize::SIZE as usize;

/// A [`Source`] that maps more memory into the heap whenever it runs out of memory.
///
/// The heap's virtual range is reserved by [`init`], but only mapped on demand.
#[derive(Debug)]
pub struct LazyHeap {
	current_end: usize,
	limit: usize,
}

impl LazyHeap {
	pub const fn new() -> Self {
		Self {
			current_end: 0,
			limit: 0,
		}
	}
}

// SAFETY: `acquire` does not allocate.
unsafe impl Source for LazyHeap {
	fn acquire<B: Binning>(talc: &mut Talc<Self, B>, layout: Layout) -> Result<(), ()> {
		let Self { current_end, limit } = talc.source;
		if current_end == 0 {
			return Err(());
		}

		// Leave room for chunk metadata and alignment padding.
		let needed = layout
			.size()
			.checked_add(layout.align() + 4 * size_of::<usize>())
			.and_then(|size| size.checked_next_multiple_of(GROW_STEP))
			.ok_or(())?;
		if needed > limit - current_end {
			return Err(());
		}
		// Map one step more than needed, so that subsequent allocations don't grow again immediately.
		let size = (needed + GROW_STEP).min(limit - current_end);

		// Printing while growing may deadlock, if the allocation was caused by a
		// user holding the console. So we switch to nonblocking prints here.
		let _nonblocking_print = console::nonblocking_print();
		let mapped = map_heap(VirtAddr::new(current_end as u64), size);
		if mapped == 0 {
			return Err(());
		}

		let heap_end = NonNull::new(ptr::with_exposed_provenance_mut(current_end)).unwrap();
		let new_end = unsafe {
			talc.extend(
				heap_end,
				ptr::with_exposed_provenance_mut(current_end + mapped),
			)
		};
		talc.source.current_end = new_end.as_ptr().expose_provenance();

		Ok(())
	}
}

/// Initializes the heap in the virtual range `start..start + size` but maps only a small part of it.
pub fn init(start: VirtAddr, size: usize) {
	const INITIAL_SIZE: usize = GROW_STEP;

	assert!(start.is_aligned_to(GROW_STEP as u64));
	let size = size.align_down(GROW_STEP);

	let initial_size = map_heap(start, INITIAL_SIZE.min(size));
	assert!(initial_size > 0, "Unable to map the initial heap");

	let mut talc = ALLOCATOR.lock();
	let end = unsafe { talc.claim(start.as_mut_ptr(), initial_size) }.unwrap();
	let end = end.as_ptr().expose_provenance();
	assert_eq!(end, start.as_usize() + initial_size);
	talc.source = LazyHeap {
		current_end: end,
		limit: start.as_usize() + size,
	};
	drop(talc);

	info!(
		"Heap is located at {start:p}..{:p} ({initial_size} Bytes mapped)",
		start + size
	);
}

/// Maps up to `size` bytes at `virt_addr` to newly allocated frames.
///
/// Returns the number of mapped bytes, which is less than `size` if physical memory runs out.
fn map_heap(virt_addr: VirtAddr, size: usize) -> usize {
	if processor::supports_2mib_pages() {
		let page_size = LargePageSize::SIZE as usize;
		paging::map_heap::<LargePageSize>(virt_addr, size / page_size)
			.map_or_else(|n| n * page_size, |()| size)
	} else {
		let page_size = BasePageSize::SIZE as usize;
		paging::map_heap::<BasePageSize>(virt_addr, size / page_size)
			.map_or_else(|n| n * page_size, |()| size)
	}
}
