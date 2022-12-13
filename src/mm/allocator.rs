//! Implementation of the HermitCore Allocator for dynamically allocating heap memory
//! in the kernel.

// This memory allocator is derived from the crate `linked-list-allocator`
// (https://github.com/phil-opp/linked-list-allocator).
// This crate is dual-licensed under MIT or the Apache License (Version 2.0).

#![allow(dead_code)]

use core::alloc::{AllocError, GlobalAlloc, Layout};
use core::ops::Deref;
use core::ptr::NonNull;
use core::{cmp, mem, ptr};

use align_address::Align;
use hermit_sync::InterruptTicketMutex;

use crate::mm::hole::{Hole, HoleList};
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

		// Bump the heap index and align it up to the next boundary.
		self.index = (self.index + layout.size()).align_up(HW_DESTRUCTIVE_INTERFERENCE_SIZE);
		if self.index >= Self::SIZE {
			Err(AllocError)
		} else {
			Ok(NonNull::new(ptr).unwrap())
		}
	}
}

/// A fixed size heap backed by a linked list of free memory blocks.
pub struct Heap {
	bootstrap_allocator: BootstrapAllocator,
	bottom: usize,
	size: usize,
	#[cfg(not(test))]
	holes: HoleList,
	#[cfg(test)]
	pub holes: HoleList,
}

impl Heap {
	/// Creates an empty heap. All allocate calls will return `None`.
	pub const fn empty() -> Heap {
		Heap {
			bootstrap_allocator: BootstrapAllocator::new(),
			bottom: 0,
			size: 0,
			holes: HoleList::empty(),
		}
	}

	/// Initializes an empty heap
	///
	/// # Unsafety
	///
	/// This function must be called at most once and must only be used on an
	/// empty heap.
	pub unsafe fn init(&mut self, heap_bottom: usize, heap_size: usize) {
		self.holes = unsafe { HoleList::new(heap_bottom, heap_size) };
		self.bottom = heap_bottom;
		self.size = heap_size;
	}

	/// Creates a new heap with the given `bottom` and `size`. The bottom address must be valid
	/// and the memory in the `[heap_bottom, heap_bottom + heap_size)` range must not be used for
	/// anything else. This function is unsafe because it can cause undefined behavior if the
	/// given address is invalid.
	pub unsafe fn new(heap_bottom: usize, heap_size: usize) -> Heap {
		Heap {
			bootstrap_allocator: BootstrapAllocator::new(),
			bottom: heap_bottom,
			size: heap_size,
			holes: unsafe { HoleList::new(heap_bottom, heap_size) },
		}
	}

	/// Allocates a chunk of the given size with the given alignment. Returns a pointer to the
	/// beginning of that chunk if it was successful. Else it returns `None`.
	/// This function scans the list of free memory blocks and uses the first block that is big
	/// enough. The runtime is in O(n) where n is the number of free blocks, but it should be
	/// reasonably fast for small allocations.
	pub fn allocate_first_fit(&mut self, layout: Layout) -> Result<NonNull<u8>, AllocError> {
		if self.bottom == 0 {
			unsafe { self.bootstrap_allocator.alloc(layout) }
		} else {
			let mut size = cmp::max(layout.size(), HoleList::min_size());
			size = (size).align_up(mem::align_of::<Hole>());
			size = (size).align_up(HW_DESTRUCTIVE_INTERFERENCE_SIZE);
			let layout = Layout::from_size_align(
				size,
				cmp::max(layout.align(), HW_DESTRUCTIVE_INTERFERENCE_SIZE),
			)
			.unwrap();

			self.holes.allocate_first_fit(layout)
		}
	}

	/// Frees the given allocation. `ptr` must be a pointer returned
	/// by a call to the `allocate_first_fit` function with identical size and alignment. Undefined
	/// behavior may occur for invalid arguments, thus this function is unsafe.
	///
	/// This function walks the list of free memory blocks and inserts the freed block at the
	/// correct place. If the freed block is adjacent to another free block, the blocks are merged
	/// again. This operation is in `O(n)` since the list needs to be sorted by address.
	pub unsafe fn deallocate(&mut self, ptr: NonNull<u8>, layout: Layout) {
		let address = ptr.as_ptr() as usize;

		// We never deallocate memory of the Bootstrap Allocator.
		// It would only increase the management burden and we wouldn't save
		// any significant amounts of memory.
		// So check if this is a pointer allocated by the System Allocator.
		if address >= kernel_end_address().as_usize() {
			let mut size = cmp::max(layout.size(), HoleList::min_size());
			size = size.align_up(mem::align_of::<Hole>());
			size = size.align_up(HW_DESTRUCTIVE_INTERFERENCE_SIZE);
			let layout = Layout::from_size_align(
				size,
				cmp::max(layout.align(), HW_DESTRUCTIVE_INTERFERENCE_SIZE),
			)
			.unwrap();

			unsafe { self.holes.deallocate(ptr, layout) };
		}
	}

	/// Returns the bottom address of the heap.
	pub fn bottom(&self) -> usize {
		self.bottom
	}

	/// Returns the size of the heap.
	pub fn size(&self) -> usize {
		self.size
	}

	/// Return the top address of the heap
	pub fn top(&self) -> usize {
		self.bottom + self.size
	}

	/// Extends the size of the heap by creating a new hole at the end
	///
	/// # Unsafety
	///
	/// The new extended area must be valid
	pub unsafe fn extend(&mut self, by: usize) {
		let top = self.top();
		let layout = Layout::from_size_align(by, 1).unwrap();
		unsafe {
			self.holes
				.deallocate(NonNull::new_unchecked(top as *mut u8), layout);
		}
		self.size += by;
	}
}

pub struct LockedAllocator(InterruptTicketMutex<Heap>);

impl LockedAllocator {
	/// Creates an empty allocator. All allocate calls will return `None`.
	pub const fn empty() -> LockedAllocator {
		LockedAllocator(InterruptTicketMutex::new(Heap::empty()))
	}
}

impl Deref for LockedAllocator {
	type Target = InterruptTicketMutex<Heap>;

	fn deref(&self) -> &InterruptTicketMutex<Heap> {
		&self.0
	}
}

/// To avoid false sharing, the global memory allocator align
/// all requests to a cache line.
unsafe impl GlobalAlloc for LockedAllocator {
	unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
		self.0
			.lock()
			.allocate_first_fit(layout)
			.ok()
			.map_or(ptr::null_mut() as *mut u8, |mut mem| unsafe {
				mem.as_mut()
			})
	}

	unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
		unsafe {
			self.0
				.lock()
				.deallocate(NonNull::new_unchecked(ptr), layout)
		}
	}
}
