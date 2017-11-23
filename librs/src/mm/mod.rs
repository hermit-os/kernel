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

pub mod allocator;

use arch;
use arch::mm::paging::{BasePageSize, PageSize, PageTableEntryFlags};


extern "C" {
	static image_size: usize;
	static kernel_start: u8;
}


/// Physical and virtual address of the first 2 MiB page that maps the kernel.
/// Can be easily accessed through kernel_start_address()
static mut KERNEL_START_ADDRESS: usize = 0;

/// Physical and virtual address of the first page after the kernel.
/// Can be easily accessed through kernel_end_address()
static mut KERNEL_END_ADDRESS: usize = 0;


pub fn kernel_start_address() -> usize {
	unsafe { KERNEL_START_ADDRESS }
}

pub fn kernel_end_address() -> usize {
	unsafe { KERNEL_END_ADDRESS }
}

pub fn init() {
	// Calculate the start and end addresses of the 2 MiB page(s) that map the kernel.
	unsafe {
		KERNEL_START_ADDRESS = align_down!(&kernel_start as *const u8 as usize, arch::mm::paging::LargePageSize::SIZE);
		KERNEL_END_ADDRESS = align_up!(&kernel_start as *const u8 as usize + image_size, arch::mm::paging::LargePageSize::SIZE);
	}

	arch::mm::init();
	unsafe { self::allocator::ALLOCATOR_INFO.switch_to_system_allocator(); }
}

pub fn allocate(size: usize) -> usize {
	assert!(size > 0);
	assert!(size & (BasePageSize::SIZE - 1) == 0, "Size is not a multiple of 4 KiB (size = {:#X})", size);

	let physical_address = arch::mm::physicalmem::allocate(size);
	let virtual_address = arch::mm::virtualmem::allocate(size);
	let count = size / BasePageSize::SIZE;
	arch::mm::paging::map::<BasePageSize>(
		virtual_address,
		physical_address,
		count,
		PageTableEntryFlags::WRITABLE | PageTableEntryFlags::EXECUTE_DISABLE,
		true
	);

	virtual_address
}

pub fn deallocate(virtual_address: usize, size: usize) {
	assert!(size > 0);
	assert!(virtual_address >= kernel_end_address(), "Virtual address {:#X} < KERNEL_END_ADDRESS", virtual_address);
	assert!(size & (BasePageSize::SIZE - 1) == 0, "Size is not a multiple of 4 KiB (size = {:#X})", size);

	let entry = arch::mm::paging::page_table_entry::<BasePageSize>(virtual_address).expect("Page is not mapped");
	arch::mm::virtualmem::deallocate(virtual_address, size);
	arch::mm::physicalmem::deallocate(entry.address(), size);
}
