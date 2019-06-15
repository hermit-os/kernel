// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

pub mod allocator;
pub mod freelist;
mod hole;

use arch;
use arch::mm::paging::{BasePageSize, PageSize, PageTableEntryFlags};
use environment;

extern "C" {
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
		KERNEL_START_ADDRESS = align_down!(&kernel_start as *const u8 as usize,
			arch::mm::paging::LargePageSize::SIZE);
		KERNEL_END_ADDRESS = align_up!(&kernel_start as *const u8 as usize + environment::get_image_size(),
			arch::mm::paging::LargePageSize::SIZE);
	}

	arch::mm::init();
	arch::mm::init_page_tables();

	let size: usize = 2*1024*1024;
	unsafe {
		::ALLOCATOR.lock().init(allocate(size, true), size);
	}
}

pub fn print_information() {
	arch::mm::physicalmem::print_information();
	arch::mm::virtualmem::print_information();
}

pub fn allocate_iomem(sz: usize) -> usize {
	let size = align_up!(sz, BasePageSize::SIZE);

	let physical_address = arch::mm::physicalmem::allocate(size);
	let virtual_address = arch::mm::virtualmem::allocate(size);

	let count = size / BasePageSize::SIZE;
	let mut flags = PageTableEntryFlags::empty();
	flags.normal().writable().execute_disable();
	arch::mm::paging::map::<BasePageSize>(virtual_address, physical_address, count, flags);

	virtual_address
}

pub fn allocate(sz: usize, execute_disable: bool) -> usize {
	let size = align_up!(sz, BasePageSize::SIZE);

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

pub fn deallocate(virtual_address: usize, sz: usize) {
	let size = align_up!(sz, BasePageSize::SIZE);

	if let Some(entry) = arch::mm::paging::get_page_table_entry::<BasePageSize>(virtual_address) {
		arch::mm::virtualmem::deallocate(virtual_address, size);
		arch::mm::physicalmem::deallocate(entry.address(), size);
	} else {
		panic!("No page table entry for virtual address {:#X}", virtual_address);
	}
}
