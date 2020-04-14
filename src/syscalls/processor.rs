// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use arch;
use core::convert::TryInto;

/** Returns the number of processors currently online. */
#[no_mangle]
pub extern "C" fn sys_get_processor_count() -> usize {
	arch::get_processor_count().try_into().unwrap()
}

/** Returns the processor frequency in MHz. */
#[no_mangle]
pub extern "C" fn sys_get_processor_frequency() -> u16 {
	arch::processor::get_frequency()
}
