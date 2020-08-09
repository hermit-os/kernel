// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

pub mod paging;
pub mod physicalmem;
pub mod virtualmem;

pub use aarch64::paging::PhysAddr;
pub use aarch64::paging::VirtAddr;

pub use self::physicalmem::init_page_tables;

pub fn init() {
	physicalmem::init();
	virtualmem::init();
}
