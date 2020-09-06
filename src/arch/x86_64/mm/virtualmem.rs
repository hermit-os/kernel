// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use core::convert::TryInto;

use crate::arch::x86_64::mm::paging::{BasePageSize, PageSize};
use crate::arch::x86_64::mm::VirtAddr;
use crate::mm;
use crate::mm::freelist::{FreeList, FreeListEntry};
use crate::synch::spinlock::*;

static KERNEL_FREE_LIST: SpinlockIrqSave<FreeList> = SpinlockIrqSave::new(FreeList::new());

pub fn init() {
	let entry = FreeListEntry::new(
		mm::kernel_end_address().as_usize(),
		kernel_heap_end().as_usize(),
	);
	KERNEL_FREE_LIST.lock().list.push_back(entry);
}

pub fn allocate(size: usize) -> Result<VirtAddr, ()> {
	assert!(size > 0);
	assert_eq!(
		size % BasePageSize::SIZE,
		0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE
	);

	Ok(VirtAddr(
		KERNEL_FREE_LIST
			.lock()
			.allocate(size, None)?
			.try_into()
			.unwrap(),
	))
}

pub fn allocate_aligned(size: usize, alignment: usize) -> Result<VirtAddr, ()> {
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

	Ok(VirtAddr(
		KERNEL_FREE_LIST
			.lock()
			.allocate(size, Some(alignment))?
			.try_into()
			.unwrap(),
	))
}

pub fn deallocate(virtual_address: VirtAddr, size: usize) {
	assert!(
		virtual_address >= VirtAddr(mm::kernel_end_address().as_u64()),
		"Virtual address {:#X} is not >= KERNEL_END_ADDRESS",
		virtual_address
	);
	assert!(
		virtual_address < kernel_heap_end(),
		"Virtual address {:#X} is not < kernel_heap_end()",
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

	KERNEL_FREE_LIST
		.lock()
		.deallocate(virtual_address.as_usize(), size);
}

/*pub fn reserve(virtual_address: VirtAddr, size: usize) {
	assert!(
		virtual_address >= VirtAddr(mm::kernel_end_address().as_u64()),
		"Virtual address {:#X} is not >= KERNEL_END_ADDRESS",
		virtual_address
	);
	assert!(
		virtual_address < kernel_heap_end(),
		"Virtual address {:#X} is not < kernel_heap_end()",
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

	let result = KERNEL_FREE_LIST
		.lock()
		.reserve(virtual_address.as_usize(), size);
	assert!(
		result.is_ok(),
		"Could not reserve {:#X} bytes of virtual memory at {:#X}",
		size,
		virtual_address
	);
}*/

pub fn print_information() {
	KERNEL_FREE_LIST
		.lock()
		.print_information(" KERNEL VIRTUAL MEMORY FREE LIST ");
}

/// End of the virtual memory address space reserved for kernel memory.
/// This also marks the start of the virtual memory address space reserved for the task heap.
/// In case of pure rust applications, we don't have a task heap.
#[cfg(not(feature = "newlib"))]
#[inline]
pub const fn kernel_heap_end() -> VirtAddr {
	VirtAddr(0x8000_0000_0000u64)
}

#[cfg(feature = "newlib")]
#[inline]
pub const fn kernel_heap_end() -> VirtAddr {
	VirtAddr(0x1_0000_0000u64)
}
