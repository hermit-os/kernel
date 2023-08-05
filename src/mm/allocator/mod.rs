//! Implementation of the HermitCore Allocator for dynamically allocating heap memory
//! in the kernel.

mod bootstrap;
mod bump;

use core::alloc::{AllocError, Allocator, GlobalAlloc, Layout};
use core::ptr;
use core::ptr::NonNull;

use align_address::Align;
use hermit_sync::InterruptTicketMutex;
use talc::{ErrOnOom, Span, Talc};

use self::bootstrap::BootstrapAllocator;
use self::bump::BumpAllocator;
use crate::HW_DESTRUCTIVE_INTERFERENCE_SIZE;

/// The global system allocator for Hermit.
struct GlobalAllocator {
	/// The bootstrap allocator, which is available immediately.
	///
	/// It allows allocations before the heap has been initalized.
	bootstrap_allocator: Option<BootstrapAllocator<BumpAllocator>>,

	/// The heap allocator.
	///
	/// This is not available immediately and must be initialized ([`Self::init`]).
	heap: Option<Talc<ErrOnOom>>,
}

impl GlobalAllocator {
	const fn empty() -> Self {
		Self {
			bootstrap_allocator: None,
			heap: None,
		}
	}

	/// Initializes the heap allocator.
	///
	/// # Safety
	///
	/// The memory starting from `heap_bottom` with a size of `heap_size`
	/// must be valid and ready to be managed and allocated from.
	unsafe fn init(&mut self, heap_bottom: *mut u8, heap_size: usize) {
		self.heap = unsafe {
			Some(Talc::with_arena(
				ErrOnOom,
				Span::from_base_size(heap_bottom, heap_size),
			))
		}
	}

	fn align_layout(layout: Layout) -> Layout {
		let size = layout.size().align_up(HW_DESTRUCTIVE_INTERFERENCE_SIZE);
		let align = layout.align().max(HW_DESTRUCTIVE_INTERFERENCE_SIZE);
		Layout::from_size_align(size, align).unwrap()
	}

	fn allocate(&mut self, layout: Layout) -> Result<NonNull<u8>, AllocError> {
		let layout = Self::align_layout(layout);
		match &mut self.heap {
			Some(heap) => unsafe { heap.malloc(layout).map_err(|_| AllocError) },
			None => self
				.bootstrap_allocator
				.get_or_insert_with(Default::default)
				.allocate(layout)
				// FIXME: Use NonNull::as_mut_ptr once `slice_ptr_get` is stabilized
				// https://github.com/rust-lang/rust/issues/74265
				.map(|ptr| NonNull::new(ptr.as_ptr() as *mut u8).unwrap()),
		}
	}

	unsafe fn deallocate(&mut self, ptr: NonNull<u8>, layout: Layout) {
		let layout = Self::align_layout(layout);
		let bootstrap_allocator = self.bootstrap_allocator.as_ref().unwrap();
		if bootstrap_allocator.manages(ptr) {
			unsafe {
				bootstrap_allocator.deallocate(ptr, layout);
			}
		} else {
			unsafe {
				self.heap.as_mut().unwrap().free(ptr, layout);
			}
		}
	}
}

pub struct LockedAllocator(InterruptTicketMutex<GlobalAllocator>);

impl LockedAllocator {
	/// Creates an empty allocator. All allocate calls will return `None`.
	pub const fn empty() -> LockedAllocator {
		LockedAllocator(InterruptTicketMutex::new(GlobalAllocator::empty()))
	}

	pub unsafe fn init(&self, heap_bottom: *mut u8, heap_size: usize) {
		unsafe {
			self.0.lock().init(heap_bottom, heap_size);
		}
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
