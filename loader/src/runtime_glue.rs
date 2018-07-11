// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//
// MIT License
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
// WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

//! Minor functions that Rust really expects to be defined by the compiler,
//! but which we need to provide manually because we're on bare metal.

#![allow(private_no_mangle_fns)]

use core::panic::PanicInfo;

// see https://users.rust-lang.org/t/psa-breaking-change-panic-fmt-language-item-removed-in-favor-of-panic-implementation/17875
#[panic_implementation]
#[no_mangle]
fn panic(info: &PanicInfo) -> ! {
	loaderlog!("PANIC: ");

	if let Some(location) = info.location() {
		loaderlog!("{}:{}: ", location.file(), location.line());
	}

	if let Some(message) = info.message() {
		loaderlog!("{}", message);
	}

	loaderlog!("\n");

	loop {
		unsafe { asm!("hlt" :::: "volatile"); }
	}
}

#[no_mangle]
#[allow(non_snake_case)]
pub fn _Unwind_Resume()
{
	loaderlog!("UNWIND!");
	loop {
		unsafe { asm!("hlt" :::: "volatile"); }
	}
}
