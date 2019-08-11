// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use arch::x86_64::mm::paging::{BasePageSize, PageSize};
use collections::Node;
use environment;
use mm;
use mm::freelist::{FreeList, FreeListEntry};
use synch::spinlock::*;

static KERNEL_FREE_LIST: SpinlockIrqSave<FreeList> = SpinlockIrqSave::new(FreeList::new());

pub fn init() {
	let entry = Node::new(FreeListEntry {
		start: mm::kernel_end_address(),
		end: kernel_heap_end(),
	});
	KERNEL_FREE_LIST.lock().list.push(entry);
}

pub fn allocate(size: usize) -> Result<usize, ()> {
	assert!(size > 0);
	assert!(
		size % BasePageSize::SIZE == 0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE
	);

	KERNEL_FREE_LIST.lock().allocate(size)
}

pub fn allocate_aligned(size: usize, alignment: usize) -> Result<usize, ()> {
	assert!(size > 0);
	assert!(alignment > 0);
	assert!(
		size % alignment == 0,
		"Size {:#X} is not a multiple of the given alignment {:#X}",
		size,
		alignment
	);
	assert!(
		alignment % BasePageSize::SIZE == 0,
		"Alignment {:#X} is not a multiple of {:#X}",
		alignment,
		BasePageSize::SIZE
	);

	KERNEL_FREE_LIST.lock().allocate_aligned(size, alignment)
}

pub fn deallocate(virtual_address: usize, size: usize) {
	assert!(
		virtual_address >= mm::kernel_end_address(),
		"Virtual address {:#X} is not >= KERNEL_END_ADDRESS",
		virtual_address
	);
	assert!(
		virtual_address < kernel_heap_end(),
		"Virtual address {:#X} is not < kernel_heap_end()",
		virtual_address
	);
	assert!(
		virtual_address % BasePageSize::SIZE == 0,
		"Virtual address {:#X} is not a multiple of {:#X}",
		virtual_address,
		BasePageSize::SIZE
	);
	assert!(size > 0);
	assert!(
		size % BasePageSize::SIZE == 0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE
	);

	KERNEL_FREE_LIST.lock().deallocate(virtual_address, size);
}

pub fn reserve(virtual_address: usize, size: usize) {
	assert!(
		virtual_address >= mm::kernel_end_address(),
		"Virtual address {:#X} is not >= KERNEL_END_ADDRESS",
		virtual_address
	);
	assert!(
		virtual_address < kernel_heap_end(),
		"Virtual address {:#X} is not < kernel_heap_end()",
		virtual_address
	);
	assert!(
		virtual_address % BasePageSize::SIZE == 0,
		"Virtual address {:#X} is not a multiple of {:#X}",
		virtual_address,
		BasePageSize::SIZE
	);
	assert!(size > 0);
	assert!(
		size % BasePageSize::SIZE == 0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE
	);

	let result = KERNEL_FREE_LIST.lock().reserve(virtual_address, size);
	assert!(
		result.is_ok(),
		"Could not reserve {:#X} bytes of virtual memory at {:#X}",
		size,
		virtual_address
	);
}

pub fn print_information() {
	KERNEL_FREE_LIST
		.lock()
		.print_information(" KERNEL VIRTUAL MEMORY FREE LIST ");
}

/// End of the virtual memory address space reserved for kernel memory.
/// This also marks the start of the virtual memory address space reserved for the task heap.
/// In case of pure rust applications, we don't have a task heap.
#[inline]
pub fn kernel_heap_end() -> usize {
	if !environment::is_pure_rust() {
		0x1_0000_0000
	} else {
		0x8000_0000_0000
	}
}

#[inline]
pub fn task_heap_start() -> usize {
	kernel_heap_end()
}

/// End of the virtual memory address space reserved for task memory (128 TiB).
/// This is the maximum contiguous virtual memory area possible with current x86-64 CPUs, which only support 48-bit
/// linear addressing (in two 47-bit areas).
#[inline]
pub const fn task_heap_end() -> usize {
	0x8000_0000_0000
}
