//! Implementation of the HermitCore Allocator for dynamically allocating heap memory
//! in the kernel.

use core::alloc::{AllocError, GlobalAlloc, Layout};
use core::ptr;
use core::ptr::NonNull;

use align_address::Align;
use hermit_sync::InterruptTicketMutex;
use linked_list_allocator::Heap;

use crate::mm::kernel_end_address;
use crate::HW_DESTRUCTIVE_INTERFERENCE_SIZE;

struct BootstrapAllocator {
	first_block: [u8; Self::SIZE],
	index: usize,
}

impl BootstrapAllocator {
	const SIZE: usize = 4096;

	const fn new() -> Self {
		Self {
			first_block: [0xCC; Self::SIZE],
			index: 0,
		}
	}

	/// An allocation using the always available Bootstrap Allocator.
	unsafe fn alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, AllocError> {
		let ptr = &mut self.first_block[self.index] as *mut u8;

		self.index += layout.size();
		if self.index >= Self::SIZE {
			Err(AllocError)
		} else {
			Ok(NonNull::new(ptr).unwrap())
		}
	}
}

struct Allocator {
	bootstrap_allocator: BootstrapAllocator,
	heap: Option<Heap>,
}

impl Allocator {
	const fn empty() -> Self {
		Self {
			bootstrap_allocator: BootstrapAllocator::new(),
			heap: None,
		}
	}

	unsafe fn init(&mut self, heap_bottom: usize, heap_size: usize) {
		self.heap = Some(unsafe { Heap::new(heap_bottom, heap_size) });
	}

	fn align_layout(layout: Layout) -> Layout {
		let size = layout.size().align_up(HW_DESTRUCTIVE_INTERFERENCE_SIZE);
		let align = layout.align().max(HW_DESTRUCTIVE_INTERFERENCE_SIZE);
		Layout::from_size_align(size, align).unwrap()
	}

	fn allocate(&mut self, layout: Layout) -> Result<NonNull<u8>, AllocError> {
		let layout = Self::align_layout(layout);
		match &mut self.heap {
			Some(heap) => heap.allocate_first_fit(layout).map_err(|()| AllocError),
			None => unsafe { self.bootstrap_allocator.alloc(layout) },
		}
	}

	unsafe fn deallocate(&mut self, ptr: NonNull<u8>, layout: Layout) {
		let layout = Self::align_layout(layout);
		if ptr.as_ptr() as usize >= kernel_end_address().as_usize() {
			unsafe { self.heap.as_mut().unwrap().deallocate(ptr, layout) }
		} else {
			// Don't deallocate from bootstrap_allocator
		}
	}
}

pub struct LockedAllocator(InterruptTicketMutex<Allocator>);

impl LockedAllocator {
	/// Creates an empty allocator. All allocate calls will return `None`.
	pub const fn empty() -> LockedAllocator {
		LockedAllocator(InterruptTicketMutex::new(Allocator::empty()))
	}

	pub unsafe fn init(&self, heap_bottom: usize, heap_size: usize) {
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
		let mut allocator = Allocator::empty();
		let layout = Layout::from_size_align(1, 1).unwrap();
		// we have 4 kbyte static memory
		assert!(allocator.allocate(layout.clone()).is_ok());

		let layout = Layout::from_size_align(0x1000, mem::align_of::<usize>());
		let addr = allocator.allocate(layout.unwrap());
		assert!(addr.is_err());
	}
}
