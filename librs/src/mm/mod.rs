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
pub mod freelist;
mod mmlock;
mod nodepool;

use arch;
use arch::mm::paging::{BasePageSize, PageSize, PageTableEntryFlags};
use environment;
use mm::mmlock::MmLock;
use mm::nodepool::NodePool;


extern "C" {
	static kernel_start: u8;
}


pub static MM_LOCK: MmLock = MmLock::new();
pub static mut POOL: NodePool = NodePool::new();


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
		KERNEL_START_ADDRESS = align_down!(&kernel_start as *const u8 as usize,
			arch::mm::paging::LargePageSize::SIZE);
		KERNEL_END_ADDRESS = align_up!(&kernel_start as *const u8 as usize + environment::get_image_size(),
			arch::mm::paging::LargePageSize::SIZE);
	}

	arch::mm::init();
	self::allocator::init();
	arch::mm::init_page_tables();
}

pub fn print_information() {
	arch::mm::physicalmem::print_information();
	arch::mm::virtualmem::print_information();
}

pub fn allocate(size: usize, execute_disable: bool) -> usize {
	let _lock = MM_LOCK.lock();

	let physical_address = arch::mm::physicalmem::allocate(size);
	let virtual_address = arch::mm::virtualmem::allocate(size);

	let count = size / BasePageSize::SIZE;
	let mut flags = PageTableEntryFlags::empty();
	flags.normal().writable();
	if execute_disable {
		flags.execute_disable();
	}
	arch::mm::paging::map::<BasePageSize>(virtual_address, physical_address, count, flags);

	virtual_address
}

pub fn deallocate(virtual_address: usize, size: usize) {
	let _lock = MM_LOCK.lock();

	if let Some(entry) = arch::mm::paging::get_page_table_entry::<BasePageSize>(virtual_address) {
		arch::mm::virtualmem::deallocate(virtual_address, size);
		arch::mm::physicalmem::deallocate(entry.address(), size);
	} else {
		panic!("No page table entry for virtual address {:#X}", virtual_address);
	}
}
