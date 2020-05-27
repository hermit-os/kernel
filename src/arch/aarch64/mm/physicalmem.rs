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

extern "C" {
	static limit: usize;
}

static mut PHYSICAL_FREE_LIST: FreeList = FreeList::new();

fn detect_from_limits() -> Result<(), ()> {
	if unsafe { limit } == 0 {
		return Err(());
	}

	let entry = Node::new(FreeListEntry {
		start: mm::kernel_end_address(),
		end: unsafe { limit },
	});
	unsafe {
		PHYSICAL_FREE_LIST.list.push(entry);
	}

	Ok(())
}

pub fn init() {
	detect_from_limits().unwrap();
}

pub fn init_page_tables() {}

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
	let result = unsafe { PHYSICAL_FREE_LIST.allocate(size) };
	assert!(
		result.is_ok(),
		"Could not allocate {:#X} bytes of physical memory",
		size
	);
	result.unwrap()
}

pub fn allocate_aligned(size: usize, alignment: usize) -> usize {
	assert!(size > 0);
	assert!(alignment > 0);
	assert_eq!(
		size % alignment,
		0,
		"Size {:#X} is not a multiple of the given alignment {:#X}",
		size,
		alignment
	);
	assert_eq!(
		alignment % BasePageSize::SIZE,
		0,
		"Alignment {:#X} is not a multiple of {:#X}",
		alignment,
		BasePageSize::SIZE
	);

	let _lock = MM_LOCK.lock();
	let result = unsafe {
		POOL.maintain();
		PHYSICAL_FREE_LIST.allocate_aligned(size, alignment)
	};
	assert!(
		result.is_ok(),
		"Could not allocate {:#X} bytes of physical memory aligned to {} bytes",
		size,
		alignment
	);
	result.unwrap()
}

/// This function must only be called from mm::deallocate!
/// Otherwise, it may fail due to an empty node pool (POOL.maintain() is called in virtualmem::deallocate)
pub fn deallocate(physical_address: usize, size: usize) {
	assert!(
		physical_address >= mm::kernel_end_address(),
		"Physical address {:#X} is not >= KERNEL_END_ADDRESS",
		physical_address
	);
	assert!(size > 0);
	assert_eq!(
		size % BasePageSize::SIZE,
		0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE
	);

	unsafe {
		PHYSICAL_FREE_LIST.deallocate(physical_address, size);
	}
}

pub fn print_information() {
	unsafe {
		PHYSICAL_FREE_LIST.print_information(" PHYSICAL MEMORY FREE LIST ");
	}
}
