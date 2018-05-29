// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//
// MIT License
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
// WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

use arch::x86_64::mm::paging::{BasePageSize, PageSize};
use collections::Node;
use mm;
use mm::freelist::{FreeList, FreeListEntry};
use mm::POOL;


static mut KERNEL_FREE_LIST: FreeList = FreeList::new();

/// End of the virtual memory address space reserved for kernel memory (4 GiB).
/// This also marks the start of the virtual memory address space reserved for the task heap.
const KERNEL_VIRTUAL_MEMORY_END: usize = 0x1_0000_0000;

/// End of the virtual memory address space reserved for task memory (128 TiB).
/// This is the maximum contiguous virtual memory area possible with current x86-64 CPUs, which only support 48-bit
/// linear addressing (in two 47-bit areas).
const TASK_VIRTUAL_MEMORY_END: usize = 0x8000_0000_0000;


pub fn init() {
	let entry = Node::new(
		FreeListEntry {
			start: mm::kernel_end_address(),
			end: KERNEL_VIRTUAL_MEMORY_END
		}
	);
	unsafe { KERNEL_FREE_LIST.list.push(entry); }
}

pub fn allocate(size: usize) -> usize {
	assert!(size > 0);
	assert!(size % BasePageSize::SIZE == 0, "Size {:#X} is not a multiple of {:#X}", size, BasePageSize::SIZE);

	let result = unsafe { KERNEL_FREE_LIST.allocate(size) };
	assert!(result.is_ok(), "Could not allocate {:#X} bytes of virtual memory", size);
	result.unwrap()
}

pub fn deallocate(virtual_address: usize, size: usize) {
	assert!(virtual_address >= mm::kernel_end_address(), "Virtual address {:#X} is not >= KERNEL_END_ADDRESS", virtual_address);
	assert!(virtual_address < KERNEL_VIRTUAL_MEMORY_END, "Virtual address {:#X} is not < KERNEL_VIRTUAL_MEMORY_END", virtual_address);
	assert!(virtual_address % BasePageSize::SIZE == 0, "Virtual address {:#X} is not a multiple of {:#X}", virtual_address, BasePageSize::SIZE);
	assert!(size > 0);
	assert!(size % BasePageSize::SIZE == 0, "Size {:#X} is not a multiple of {:#X}", size, BasePageSize::SIZE);

	unsafe {
		POOL.maintain();
		KERNEL_FREE_LIST.deallocate(virtual_address, size);
	}
}

pub fn reserve(virtual_address: usize, size: usize) {
	assert!(virtual_address >= mm::kernel_end_address(), "Virtual address {:#X} is not >= KERNEL_END_ADDRESS", virtual_address);
	assert!(virtual_address < KERNEL_VIRTUAL_MEMORY_END, "Virtual address {:#X} is not < KERNEL_VIRTUAL_MEMORY_END", virtual_address);
	assert!(virtual_address % BasePageSize::SIZE == 0, "Virtual address {:#X} is not a multiple of {:#X}", virtual_address, BasePageSize::SIZE);
	assert!(size > 0);
	assert!(size % BasePageSize::SIZE == 0, "Size {:#X} is not a multiple of {:#X}", size, BasePageSize::SIZE);

	let result = unsafe {
		POOL.maintain();
		KERNEL_FREE_LIST.reserve(virtual_address, size)
	};
	assert!(result.is_ok(), "Could not reserve {:#X} bytes of virtual memory at {:#X}", size, virtual_address);
}

pub fn print_information() {
	unsafe { KERNEL_FREE_LIST.print_information(" KERNEL VIRTUAL MEMORY FREE LIST "); }
}

#[inline]
pub fn task_heap_start() -> usize {
	KERNEL_VIRTUAL_MEMORY_END
}

#[inline]
pub fn task_heap_end() -> usize {
	TASK_VIRTUAL_MEMORY_END
}
