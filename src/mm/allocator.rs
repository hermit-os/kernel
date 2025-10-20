//! Implementation of the Hermit Allocator for dynamically allocating heap memory
//! in the kernel.

use core::alloc::{GlobalAlloc, Layout};

use hermit_sync::RawInterruptTicketMutex;
#[cfg(not(feature = "balloon"))]
use talc::ErrOnOom;
use talc::{Span, Talc, Talck};

#[cfg(feature = "balloon")]
use crate::drivers::balloon::oom::DeflateBalloonOnOom;

#[cfg(not(feature = "balloon"))]
pub(crate) type HermitOomHandler = ErrOnOom;
#[cfg(feature = "balloon")]
pub(crate) type HermitOomHandler = DeflateBalloonOnOom;

pub struct LockedAllocator(Talck<RawInterruptTicketMutex, HermitOomHandler>);

impl LockedAllocator {
	#[cfg(not(feature = "balloon"))]
	pub const fn new() -> Self {
		Self(Talc::new(ErrOnOom).lock())
	}

	/// # Safety
	/// May only be used to construct the allocator that will be used as the
	/// global allocator.
	#[cfg(feature = "balloon")]
	pub const unsafe fn new() -> Self {
		// SAFETY: We pass on the requirement of usage being restricted to only
		//         the one global allocator to our caller.
		Self(Talc::new(unsafe { DeflateBalloonOnOom::new() }).lock())
	}

	#[inline]
	fn align_layout(layout: Layout) -> Layout {
		let align = layout
			.align()
			.max(core::mem::align_of::<crossbeam_utils::CachePadded<u8>>());
		Layout::from_size_align(layout.size(), align).unwrap()
	}

	pub unsafe fn init(&self, heap_bottom: *mut u8, heap_size: usize) {
		let arena = Span::from_base_size(heap_bottom, heap_size);
		unsafe {
			self.0.lock().claim(arena).unwrap();
		}
	}

	pub(crate) fn inner(&self) -> &Talck<RawInterruptTicketMutex, HermitOomHandler> {
		&self.0
	}
}

/// To avoid false sharing, the global memory allocator align
/// all requests to a cache line.
unsafe impl GlobalAlloc for LockedAllocator {
	unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
		let layout = Self::align_layout(layout);
		unsafe { self.0.alloc(layout) }
	}

	unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
		let layout = Self::align_layout(layout);
		unsafe { self.0.dealloc(ptr, layout) }
	}

	unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
		let layout = Self::align_layout(layout);
		unsafe { self.0.alloc_zeroed(layout) }
	}

	unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
		let layout = Self::align_layout(layout);
		unsafe { self.0.realloc(ptr, layout, new_size) }
	}
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
	use core::mem;

	use super::*;

	#[test]
	fn empty() {
		const ARENA_SIZE: usize = 0x1000;
		let mut arena: [u8; ARENA_SIZE] = [0; ARENA_SIZE];
		let allocator: LockedAllocator = LockedAllocator::new();
		unsafe {
			allocator.init(arena.as_mut_ptr(), ARENA_SIZE);
		}

		let layout = Layout::from_size_align(1, 1).unwrap();
		// we have 4 kbyte  memory
		assert!(unsafe { !allocator.alloc(layout.clone()).is_null() });

		let layout = Layout::from_size_align(0x1000, mem::align_of::<usize>()).unwrap();
		let addr = unsafe { allocator.alloc(layout) };
		assert!(addr.is_null());
	}
}
