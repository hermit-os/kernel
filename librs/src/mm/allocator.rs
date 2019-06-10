// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! Implementation of the HermitCore Allocator for dynamically allocating heap memory
//! in the kernel.
//!
//! The data structures used to manage heap memory require dynamic memory allocations
//! themselves. To solve this chicken-egg problem, HermitCore first uses a
//! "Bootstrap Allocator". This is a simple single-threaded implementation using some
//! preallocated space within KERNEL_START_ADDRESS and KERNEL_END_ADDRESS, along with an
//! index variable. Freed memory is never reused, but this can be neglected for bootstrapping.
//!
//! As soon as all required data structures have been set up, the "System Allocator" is used.
//! It manages all memory >= KERNEL_END_ADDRESS.

use alloc::alloc::Layout;
use core::alloc::GlobalAlloc;
use arch::mm::paging::{BasePageSize, PageSize};
use mm;

/// Size of the preallocated space for the Bootstrap Allocator.
const BOOTSTRAP_HEAP_SIZE: usize = 4096;

/// Alignment of pointers returned by the Bootstrap Allocator.
/// Note that you also have to align the HermitAllocatorInfo structure!
const BOOTSTRAP_HEAP_ALIGNMENT: usize = 8;


/// The HermitAllocator structure is immutable, so we need this helper structure
/// for our allocator information.
#[repr(align(8))]
pub struct HermitAllocatorInfo {
	heap: [u8; BOOTSTRAP_HEAP_SIZE],
	index: usize,
	is_bootstrapping: bool,
}

impl HermitAllocatorInfo {
	const fn new() -> Self {
		Self {
			heap: [0xCC; BOOTSTRAP_HEAP_SIZE],
			index: 0,
			is_bootstrapping: true,
		}
	}

	pub fn switch_to_system_allocator(&mut self) {
		trace!("Switching to the System Allocator");
		self.is_bootstrapping = false;
	}
}

static mut ALLOCATOR_INFO: HermitAllocatorInfo = HermitAllocatorInfo::new();

pub struct HermitAllocator;

unsafe impl<'a> GlobalAlloc for &'a HermitAllocator {
	unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
		if ALLOCATOR_INFO.is_bootstrapping {
			alloc_bootstrap(layout)
		} else {
			alloc_system(layout)
		}
	}

	unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
		let address = ptr as usize;

		// We never deallocate memory of the Bootstrap Allocator.
		// It would only increase the management burden and we wouldn't save
		// any significant amounts of memory.
		// So check if this is a pointer allocated by the System Allocator.
		if address >= mm::kernel_end_address() {
			dealloc_system(address, layout);
		}
	}
}

/// An allocation using the always available Bootstrap Allocator.
unsafe fn alloc_bootstrap(layout: Layout) -> *mut u8 {
	let ptr = &mut ALLOCATOR_INFO.heap[ALLOCATOR_INFO.index] as *mut u8;
	trace!("Allocating {} bytes at {:#X} using the Bootstrap Allocator", layout.size(), ptr as usize);

	// Bump the heap index and align it up to the next BOOTSTRAP_HEAP_ALIGNMENT boundary.
	ALLOCATOR_INFO.index = align_up!(ALLOCATOR_INFO.index + layout.size(), BOOTSTRAP_HEAP_ALIGNMENT);
	if ALLOCATOR_INFO.index >= BOOTSTRAP_HEAP_SIZE {
		panic!("Bootstrap Allocator Overflow! Increase BOOTSTRAP_HEAP_SIZE.");
	}

	ptr
}

/// An allocation using the initialized System Allocator.
fn alloc_system(layout: Layout) -> *mut u8 {
	let size = align_up!(layout.size(), BasePageSize::SIZE);
	let virtual_address = mm::allocate(size, true);

	trace!("Allocated {} bytes at {:#X} using the System Allocator", layout.size(), virtual_address);

	virtual_address as *mut u8
}

/// A deallocation using the initialized System Allocator.
fn dealloc_system(virtual_address: usize, layout: Layout) {
	trace!("Deallocating {} bytes at {:#X} using the System Allocator", layout.size(), virtual_address);

	let size = align_up!(layout.size(), BasePageSize::SIZE);
	mm::deallocate(virtual_address, size);
}

pub fn init() {
	unsafe { ALLOCATOR_INFO.switch_to_system_allocator(); }
}
