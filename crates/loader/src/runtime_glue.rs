// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! Minor functions that Rust really expects to be defined by the compiler,
//! but which we need to provide manually because we're on bare metal.

use core::panic::PanicInfo;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
	loaderlog!("PANIC: ");

	if let Some(location) = info.location() {
		loaderlog!("{}:{}: ", location.file(), location.line());
	}

	if let Some(message) = info.message() {
		loaderlog!("{}", message);
	}

	loaderlog!("\n");

	loop {}
}
