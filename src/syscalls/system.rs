// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::arch;

extern "C" fn __sys_getpagesize() -> i32 {
	arch::mm::paging::get_application_page_size() as i32
}

#[no_mangle]
pub extern "C" fn sys_getpagesize() -> i32 {
	kernel_function!(__sys_getpagesize())
}
