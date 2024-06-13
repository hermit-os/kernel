//! Implementation of the Hermit Allocator in the loader

mod bootstrap;

use core::ptr;
use core::ptr::NonNull;

use allocator_api2::alloc::{AllocError, Allocator, GlobalAlloc, Layout};
use one_shot_mutex::OneShotMutex;

use self::bootstrap::BootstrapAllocator;
use crate::bump_allocator::BumpAllocator;

/// The global system allocator for Hermit.
struct GlobalAllocator {
	/// The bootstrap allocator, which is available immediately.
	///
	/// It allows allocations before the heap has been initalized.
	bootstrap_allocator: Option<BootstrapAllocator<BumpAllocator>>,
}

impl GlobalAllocator {
	const fn empty() -> Self {
		Self {
			bootstrap_allocator: None,
		}
	}

	fn align_layout(layout: Layout) -> Layout {
		let size = layout.size();
		let align = layout.align();
		Layout::from_size_align(size, align).unwrap()
	}

	fn allocate(&mut self, layout: Layout) -> Result<NonNull<u8>, AllocError> {
		let layout = Self::align_layout(layout);
		self.bootstrap_allocator
			.get_or_insert_with(Default::default)
			.allocate(layout)
			// FIXME: Use NonNull::as_mut_ptr once `slice_ptr_get` is stabilized
			// https://github.com/rust-lang/rust/issues/74265
			.map(|ptr| NonNull::new(ptr.as_ptr() as *mut u8).unwrap())
	}

	unsafe fn deallocate(&mut self, ptr: NonNull<u8>, layout: Layout) {
		let layout = Self::align_layout(layout);
		let bootstrap_allocator = self.bootstrap_allocator.as_ref().unwrap();
		assert!(bootstrap_allocator.manages(ptr));
		unsafe {
			bootstrap_allocator.deallocate(ptr, layout);
		}
	}
}

pub struct LockedAllocator(OneShotMutex<GlobalAllocator>);

impl LockedAllocator {
	/// Creates an empty allocator. All allocate calls will return `None`.
	pub const fn empty() -> LockedAllocator {
		LockedAllocator(OneShotMutex::new(GlobalAllocator::empty()))
	}
}

/// To avoid false sharing, the global memory allocator align
/// all requests to a cache line.
unsafe impl GlobalAlloc for LockedAllocator {
	unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
		self.0
			.lock()
			.allocate(layout)
			.ok()
			.map_or(ptr::null_mut(), |allocation| allocation.as_ptr())
	}

	unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
		unsafe {
			self.0
				.lock()
				.deallocate(NonNull::new_unchecked(ptr), layout)
		}
	}
}

#[global_allocator]
static ALLOCATOR: LockedAllocator = LockedAllocator::empty();

#[cfg(all(test, not(target_os = "none")))]
mod tests {
	use core::mem;

	use super::*;

	#[test]
	fn empty() {
		let mut allocator = GlobalAllocator::empty();
		let layout = Layout::from_size_align(1, 1).unwrap();
		// we have 4 kbyte static memory
		assert!(allocator.allocate(layout.clone()).is_ok());

		let layout = Layout::from_size_align(0x1000, mem::align_of::<usize>());
		let addr = allocator.allocate(layout.unwrap());
		assert!(addr.is_err());
	}
}
