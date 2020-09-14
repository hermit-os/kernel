// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

pub mod paging;
pub mod physicalmem;
pub mod virtualmem;

pub use self::paging::init_page_tables;
use core::mem;
use core::slice;

pub use x86::bits64::paging::PAddr as PhysAddr;
pub use x86::bits64::paging::VAddr as VirtAddr;

fn paddr_to_slice<'a>(p: multiboot::PAddr, sz: usize) -> Option<&'a [u8]> {
	unsafe {
		let ptr = mem::transmute(p);
		Some(slice::from_raw_parts(ptr, sz))
	}
}

pub fn init() {
	paging::init();
	physicalmem::init();
	virtualmem::init();
}
