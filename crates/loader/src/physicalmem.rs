// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::arch::paging::{BasePageSize, PageSize};

static mut CURRENT_ADDRESS: usize = 0;

pub fn init(address: usize) {
	unsafe {
		CURRENT_ADDRESS = address;
	}
}

pub fn allocate(size: usize) -> usize {
	assert!(size > 0);
	assert!(
		size % BasePageSize::SIZE == 0,
		"Size {:#X} is a multiple of {:#X}",
		size,
		BasePageSize::SIZE
	);

	unsafe {
		assert!(CURRENT_ADDRESS > 0, "Trying to allocate physical memory before the Physical Memory Manager has been initialized");
		let address = CURRENT_ADDRESS;
		CURRENT_ADDRESS += size;
		address
	}
}
