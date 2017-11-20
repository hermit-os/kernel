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
use collections::{FreeList, FreeListEntry};
use mm;
use synch::spinlock::*;


static KERNEL_FREE_LIST: SpinlockIrqSave<FreeList> = SpinlockIrqSave::new(FreeList::new());

/// End of the virtual memory address space reserved for kernel memory (1 GiB).
/// This also marks the start of the virtual memory address space reserved for the task heap.
const KERNEL_VIRTUAL_MEMORY_END: usize = 0x4000_0000;

/// End of the virtual memory address space reserved for task memory (128 TiB).
/// This is the maximum contiguous virtual memory area possible with current x86-64 CPUs, which only support 48-bit
/// linear addressing (in two 47-bit areas).
const TASK_VIRTUAL_MEMORY_END: usize = 0x8000_0000_0000;


pub fn init() {
	let entry = FreeListEntry {
		start: mm::kernel_end_address(),
		end: KERNEL_VIRTUAL_MEMORY_END
	};
	KERNEL_FREE_LIST.lock().list.push(entry);
}

pub fn allocate(size: usize) -> usize {
	assert!(size & (BasePageSize::SIZE - 1) == 0, "Size is not a multiple of 4 KiB (size = {:#X})", size);

	let result = KERNEL_FREE_LIST.lock().allocate(size);
	assert!(result.is_ok(), "Could not allocate {:#X} bytes of virtual memory", size);
	result.unwrap()
}

pub fn deallocate(virtual_address: usize, size: usize) {
	assert!(virtual_address >= mm::kernel_end_address(), "Virtual address {:#X} < KERNEL_END_ADDRESS", virtual_address);
	assert!(virtual_address < KERNEL_VIRTUAL_MEMORY_END, "Virtual address {:#X} >= KERNEL_VIRTUAL_MEMORY_END", virtual_address);
	assert!(size & (BasePageSize::SIZE - 1) == 0, "Size is not a multiple of 4 KiB (size = {:#X})", size);

	let result = KERNEL_FREE_LIST.lock().deallocate(virtual_address, size);
	assert!(result.is_ok(), "Could not deallocate virtual memory address {:#X} with size {:#X}", virtual_address, size);
}
