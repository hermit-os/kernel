// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//                    Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

#[macro_export]
macro_rules! align_down {
	($value:expr, $alignment:expr) => {
		($value) & !($alignment - 1)
	};
}

#[macro_export]
macro_rules! align_up {
	($value:expr, $alignment:expr) => {
		$crate::align_down!($value + ($alignment - 1), $alignment)
	};
}

/// Print formatted text to our console.
///
/// From http://blog.phil-opp.com/rust-os/printing-to-screen.html, but tweaked
/// for HermitCore.
#[macro_export]
macro_rules! print {
	($($arg:tt)+) => ({
		$crate::_print(format_args!($($arg)*));
	});
}

/// Print formatted text to our console, followed by a newline.
#[macro_export]
macro_rules! println {
	() => ($crate::print!("\n"));
	($($arg:tt)+) => ($crate::print!("{}\n", format_args!($($arg)+)));
}

#[macro_export]
macro_rules! kernel_function {
	// FIXME: Actually switch to kernel stack
	// See: https://github.com/hermitcore/libhermit-rs/issues/250
	($f:ident()) => {
		$f()
	};

	($f:ident($arg1:expr)) => {
		$f($arg1)
	};

	($f:ident($arg1:expr, $arg2:expr)) => {
		$f($arg1, $arg2)
	};

	($f:ident($arg1:expr, $arg2:expr, $arg3:expr)) => {
		$f($arg1, $arg2, $arg3)
	};

	($f:ident($arg1:expr, $arg2:expr, $arg3:expr, $arg4:expr)) => {
		$f($arg1, $arg2, $arg3, $arg4)
	};

	($f:ident($arg1:expr, $arg2:expr, $arg3:expr, $arg4:expr, $arg5:expr)) => {
		$f($arg1, $arg2, $arg3, $arg4, $arg5)
	};

	($f:ident($arg1:expr, $arg2:expr, $arg3:expr, $arg4:expr, $arg5:expr, $arg6:expr)) => {
		$f($arg1, $arg2, $arg3, $arg4, $arg5, $arg6)
	};
}
