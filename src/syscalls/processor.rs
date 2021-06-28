// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::arch::get_processor_count;
use core::convert::TryInto;

extern "C" fn __sys_get_processor_count() -> usize {
	get_processor_count().try_into().unwrap()
}

/** Returns the number of processors currently online. */
#[no_mangle]
pub extern "C" fn sys_get_processor_count() -> usize {
	kernel_function!(__sys_get_processor_count())
}

extern "C" fn __sys_get_processor_frequency() -> u16 {
	crate::arch::processor::get_frequency()
}

/** Returns the processor frequency in MHz. */
#[no_mangle]
pub extern "C" fn sys_get_processor_frequency() -> u16 {
	kernel_function!(__sys_get_processor_frequency())
}
