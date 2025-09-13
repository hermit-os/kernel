//! A simple bump memory allocator.

use alloc::alloc::Layout;
use core::alloc::GlobalAlloc;
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use align_address::Align;
use log::{trace, warn};

/// A simple bump allocator.
pub(crate) struct BumpAllocator {
	mem: AtomicPtr<u8>,
	size: AtomicUsize,
	next: AtomicUsize,
	// For stats:
	total_waste: AtomicUsize,
	waste_cnt: AtomicUsize,
}

impl BumpAllocator {
	pub const fn new() -> Self {
		Self {
			mem: AtomicPtr::<u8>::new(core::ptr::null_mut()),
			size: AtomicUsize::new(0),
			next: AtomicUsize::new(0),
			total_waste: AtomicUsize::new(0),
			waste_cnt: AtomicUsize::new(0),
		}
	}

	pub fn init(&self, mem: *mut u8, size: usize) {
		debug!("Initializing bump allocator at {mem:#p} with {size:#x} bytes");
		self.mem.store(mem, Ordering::Relaxed);
		self.size.store(size, Ordering::Relaxed);
	}
}

unsafe impl GlobalAlloc for BumpAllocator {
	unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
		trace!("Bump-allocating {:?}", layout);
		let size = layout.size().align_up(layout.align());
		let index = self.next.fetch_add(size, Ordering::Relaxed);

		if index + size <= self.size.load(Ordering::Relaxed) {
			unsafe { self.mem.load(Ordering::Relaxed).add(index) }
		} else {
			warn!("Bump allocator overflow!");
			ptr::null_mut()
		}
	}

	unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
		let waste = self.total_waste.fetch_add(layout.size(), Ordering::Relaxed);
		let cnt = self.waste_cnt.fetch_add(1, Ordering::Relaxed);
		if cnt % (512 * 64) == 511 {
			debug!(
				"Total bump allocator memory waste: {:#x} Bytes",
				waste + layout.size()
			);
		}
		// This does nothing, since this allocator does not support deallocation.
		trace!("Trying to deallocate {:?} at {:p}", layout, ptr);
	}
}

impl Drop for BumpAllocator {
	fn drop(&mut self) {
		panic!("BumpAllocator should not be dropped")
	}
}
