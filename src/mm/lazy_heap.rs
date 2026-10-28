use core::alloc::{GlobalAlloc, Layout};
use core::ops::DerefMut;
use core::ptr::{self, NonNull};

use align_address::Align;
use hermit_sync::{InterruptTicketMutex, RawInterruptTicketMutex};
use memory_addresses::VirtAddr;
use talc::TalcLock;
use talc::base::Talc;
use talc::base::binning::DefaultBinning;
use talc::source::Manual;

use super::ALLOCATOR;
use crate::arch::kernel::processor;
use crate::arch::mm::paging::{self, BasePageSize, LargePageSize, PageSize};
use crate::console;

const GROW_STEP: usize = LargePageSize::SIZE as usize;

/// The virtual range of the heap that is not yet fully mapped.
#[derive(Debug)]
struct HeapRange {
	current_end: usize,
	limit: usize,
}

/// A global allocator that maps more memory whenever the heap runs out of memory.
pub struct LazyHeap {
	/// The underlying allocator
	talc: TalcLock<RawInterruptTicketMutex, Manual>,
	/// The current size of the heap. The Mutex also prevents simultaneous grows
	heap_range: InterruptTicketMutex<HeapRange>,
}

impl LazyHeap {
	pub const fn new() -> Self {
		Self {
			talc: TalcLock::new(Manual),
			heap_range: InterruptTicketMutex::new(HeapRange {
				current_end: 0,
				limit: 0,
			}),
		}
	}

	/// Locks the underlying allocator.
	pub fn lock(&self) -> impl DerefMut<Target = Talc<Manual, DefaultBinning>> + '_ {
		self.talc.lock()
	}

	fn try_alloc(&self, layout: Layout) -> *mut u8 {
		unsafe { self.talc.lock().try_allocate(layout) }.map_or(ptr::null_mut(), NonNull::as_ptr)
	}

	/// Maps more memory into the heap, so that an allocation of `layout` fits.
	///
	/// Returns `false` if the heap cannot grow any further.
	///
	/// Note: Nothing in this function should allocate, as that bears the risk of
	/// a deadlock if a second grow is triggered!
	fn grow(&self, heap_range: &mut HeapRange, layout: Layout) -> bool {
		let HeapRange { current_end, limit } = *heap_range;
		if current_end == 0 {
			return false;
		}

		// Leave room for chunk metadata and alignment padding.
		let Some(needed) = layout
			.size()
			.checked_add(layout.align() + 4 * size_of::<usize>())
			.and_then(|size| size.checked_next_multiple_of(GROW_STEP))
		else {
			return false;
		};
		if needed > limit - current_end {
			return false;
		}
		// Map one step more than needed, so that subsequent allocations don't grow again immediately.
		let size = (needed + GROW_STEP).min(limit - current_end);

		// The logs in map_heap must not have allocating arguments.
		let mapped = map_heap(VirtAddr::new(current_end as u64), size);
		if mapped == 0 {
			return false;
		}

		let heap_end = NonNull::new(ptr::with_exposed_provenance_mut(current_end)).unwrap();
		let new_end = unsafe {
			self.talc.lock().extend(
				heap_end,
				ptr::with_exposed_provenance_mut(current_end + mapped),
			)
		};
		heap_range.current_end = new_end.as_ptr().expose_provenance();

		true
	}
}

unsafe impl GlobalAlloc for LazyHeap {
	unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
		// The heap grows without holding the talc lock, so other cores can keep
		// using the heap and try allocations during the growth.
		let ptr = self.try_alloc(layout);
		if !ptr.is_null() {
			return ptr;
		}

		let mut heap_range = self.heap_range.lock();

		// Another core might have grown the heap in the meantime.
		let ptr = self.try_alloc(layout);
		if !ptr.is_null() {
			return ptr;
		}

		// Printing while growing may deadlock, if the allocation was caused by a
		// user holding the console. So we switch to nonblocking prints here.
		let _nonblocking_print = console::nonblocking_print();
		if !self.grow(&mut heap_range, layout) {
			return ptr::null_mut();
		}
		self.try_alloc(layout)
	}

	unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
		unsafe {
			self.talc.lock().deallocate(ptr, layout);
		}
	}

	unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
		if unsafe { self.talc.lock().try_realloc_in_place(ptr, layout, new_size) } {
			return ptr;
		}

		// Shrinking always succeeds in place, so `new_size` is larger than `layout.size()`.
		let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
		let new_ptr = unsafe { self.alloc(new_layout) };
		if !new_ptr.is_null() {
			unsafe {
				new_ptr.copy_from_nonoverlapping(ptr, layout.size());
				self.dealloc(ptr, layout);
			}
		}
		new_ptr
	}
}

/// Initializes the heap in the virtual range `start..start + size` but maps only a small part of it.
pub fn init(start: VirtAddr, size: usize) {
	const INITIAL_SIZE: usize = GROW_STEP;

	assert!(start.is_aligned_to(GROW_STEP as u64));
	let size = size.align_down(GROW_STEP);

	let initial_size = map_heap(start, INITIAL_SIZE.min(size));
	assert!(initial_size > 0, "Unable to map the initial heap");

	let mut heap_range = ALLOCATOR.heap_range.lock();
	let end = unsafe { ALLOCATOR.lock().claim(start.as_mut_ptr(), initial_size) }.unwrap();
	let end = end.as_ptr().expose_provenance();
	assert_eq!(end, start.as_usize() + initial_size);
	*heap_range = HeapRange {
		current_end: end,
		limit: start.as_usize() + size,
	};
	drop(heap_range);

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
