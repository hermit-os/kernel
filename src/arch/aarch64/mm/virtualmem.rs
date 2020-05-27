// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::arch::aarch64::mm::paging::{BasePageSize, PageSize};
use crate::collections::Node;
use crate::mm;
use crate::mm::freelist::{FreeList, FreeListEntry};
use crate::mm::{MM_LOCK, POOL};

static mut KERNEL_FREE_LIST: FreeList = FreeList::new();

/// End of the virtual memory address space reserved for kernel memory (4 GiB).
/// This also marks the start of the virtual memory address space reserved for the task heap.
const KERNEL_VIRTUAL_MEMORY_END: usize = 0x1_0000_0000;

/// End of the virtual memory address space reserved for task memory (128 TiB).
/// This is the maximum contiguous virtual memory area possible with current x86-64 CPUs, which only support 48-bit
/// linear addressing (in two 47-bit areas).
const TASK_VIRTUAL_MEMORY_END: usize = 0x8000_0000_0000;

pub fn init() {
	let entry = Node::new(FreeListEntry {
		start: mm::kernel_end_address(),
		end: KERNEL_VIRTUAL_MEMORY_END,
	});
	unsafe {
		KERNEL_FREE_LIST.list.push(entry);
	}
}

pub fn allocate(size: usize) -> usize {
	assert!(size > 0);
	assert_eq!(
		size % BasePageSize::SIZE,
		0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE
	);

	let _lock = MM_LOCK.lock();
	let result = unsafe { KERNEL_FREE_LIST.allocate(size) };
	assert!(
		result.is_ok(),
		"Could not allocate {:#X} bytes of virtual memory",
		size
	);
	result.unwrap()
}

pub fn deallocate(virtual_address: usize, size: usize) {
	assert!(
		virtual_address >= mm::kernel_end_address(),
		"Virtual address {:#X} is not >= KERNEL_END_ADDRESS",
		virtual_address
	);
	assert!(
		virtual_address < KERNEL_VIRTUAL_MEMORY_END,
		"Virtual address {:#X} is not < KERNEL_VIRTUAL_MEMORY_END",
		virtual_address
	);
	assert_eq!(
		virtual_address % BasePageSize::SIZE,
		0,
		"Virtual address {:#X} is not a multiple of {:#X}",
		virtual_address,
		BasePageSize::SIZE
	);
	assert!(size > 0);
	assert_eq!(
		size % BasePageSize::SIZE,
		0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE
	);

	let _lock = MM_LOCK.lock();
	unsafe {
		POOL.maintain();
		KERNEL_FREE_LIST.deallocate(virtual_address, size);
	}
}

pub fn reserve(virtual_address: usize, size: usize) {
	assert!(
		virtual_address >= mm::kernel_end_address(),
		"Virtual address {:#X} is not >= KERNEL_END_ADDRESS",
		virtual_address
	);
	assert!(
		virtual_address < KERNEL_VIRTUAL_MEMORY_END,
		"Virtual address {:#X} is not < KERNEL_VIRTUAL_MEMORY_END",
		virtual_address
	);
	assert_eq!(
		virtual_address % BasePageSize::SIZE,
		0,
		"Virtual address {:#X} is not a multiple of {:#X}",
		virtual_address,
		BasePageSize::SIZE
	);
	assert!(size > 0);
	assert_eq!(
		size % BasePageSize::SIZE,
		0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE
	);

	let _lock = MM_LOCK.lock();
	let result = unsafe {
		POOL.maintain();
		KERNEL_FREE_LIST.reserve(virtual_address, size)
	};
	assert!(
		result.is_ok(),
		"Could not reserve {:#X} bytes of virtual memory at {:#X}",
		size,
		virtual_address
	);
}

pub fn print_information() {
	unsafe {
		KERNEL_FREE_LIST.print_information(" KERNEL VIRTUAL MEMORY FREE LIST ");
	}
}

#[inline]
pub fn task_heap_start() -> usize {
	KERNEL_VIRTUAL_MEMORY_END
}

#[inline]
pub fn task_heap_end() -> usize {
	TASK_VIRTUAL_MEMORY_END
}
