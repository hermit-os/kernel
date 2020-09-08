// Copyright (c) 2018 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

pub mod allocator;

use core::alloc::Layout;

#[cfg(not(test))]
#[lang = "oom"]
#[no_mangle]
pub fn rust_oom(layout: Layout) -> ! {
	println!(
		"[!!!OOM!!!] Memory allocation of {} bytes failed",
		layout.size()
	);

	loop {}
}
