// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use arch::x86_64::mm::paging::{BasePageSize, PageSize};
use arch::x86_64::kernel::{get_limit,get_mbinfo};
use collections::Node;
use hermit_multiboot::Multiboot;
use mm;
use mm::freelist::{FreeList, FreeListEntry};
use synch::spinlock::*;

static PHYSICAL_FREE_LIST: SpinlockIrqSave<FreeList> = SpinlockIrqSave::new(FreeList::new());


fn detect_from_multiboot_info() -> Result<(), ()> {
	let mb_info = get_mbinfo();
	if mb_info == 0 {
		return Err(());
	}

	let mb = unsafe { Multiboot::new(mb_info) };
	let all_regions = mb.memory_map().expect("Could not find a memory map in the Multiboot information");
	let ram_regions = all_regions.filter(|m|
		m.is_available() &&
		m.base_address() + m.length() > mm::kernel_end_address()
	);
	let mut found_ram = false;

	for m in ram_regions {
		found_ram = true;

		let start_address = if m.base_address() <= mm::kernel_start_address() {
			mm::kernel_end_address()
		} else {
			m.base_address()
		};

		let entry = Node::new(
			FreeListEntry {
				start: start_address,
				end: m.base_address() + m.length()
			}
		);
		PHYSICAL_FREE_LIST.lock().list.push(entry);
	}

	assert!(found_ram, "Could not find any available RAM in the Multiboot Memory Map");
	Ok(())
}

fn detect_from_limits() -> Result<(), ()> {
	let limit = get_limit();
	if limit == 0 {
		return Err(());
	}

	let entry = Node::new(
		FreeListEntry {
			start: mm::kernel_end_address(),
			end: limit
		}
	);
	PHYSICAL_FREE_LIST.lock().list.push(entry);

	Ok(())
}

pub fn init() {
	detect_from_multiboot_info()
		.or_else(|_e| detect_from_limits())
		.unwrap();
}

pub fn allocate(size: usize) -> usize {
	assert!(size > 0);
	assert!(size % BasePageSize::SIZE == 0, "Size {:#X} is not a multiple of {:#X}", size, BasePageSize::SIZE);

	let result = PHYSICAL_FREE_LIST.lock().allocate(size);
	assert!(result.is_ok(), "Could not allocate {:#X} bytes of physical memory", size);
	result.unwrap()
}

pub fn allocate_aligned(size: usize, alignment: usize) -> usize {
	assert!(size > 0);
	assert!(alignment > 0);
	assert!(size % alignment == 0, "Size {:#X} is not a multiple of the given alignment {:#X}", size, alignment);
	assert!(alignment % BasePageSize::SIZE == 0, "Alignment {:#X} is not a multiple of {:#X}", alignment, BasePageSize::SIZE);

	let result = PHYSICAL_FREE_LIST.lock().allocate_aligned(size, alignment);
	assert!(result.is_ok(), "Could not allocate {:#X} bytes of physical memory aligned to {} bytes", size, alignment);
	result.unwrap()
}

/// This function must only be called from mm::deallocate!
/// Otherwise, it may fail due to an empty node pool (POOL.maintain() is called in virtualmem::deallocate)
pub fn deallocate(physical_address: usize, size: usize) {
	assert!(physical_address >= mm::kernel_end_address(), "Physical address {:#X} is not >= KERNEL_END_ADDRESS", physical_address);
	assert!(size > 0);
	assert!(size % BasePageSize::SIZE == 0, "Size {:#X} is not a multiple of {:#X}", size, BasePageSize::SIZE);

	PHYSICAL_FREE_LIST.lock().deallocate(physical_address, size);
}

pub fn print_information() {
	PHYSICAL_FREE_LIST.lock().print_information(" PHYSICAL MEMORY FREE LIST ");
}