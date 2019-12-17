// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//                    Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

macro_rules! align_down {
	($value:expr, $alignment:expr) => {
		$value & !($alignment - 1)
	};
}

macro_rules! align_up {
	($value:expr, $alignment:expr) => {
		align_down!($value + ($alignment - 1), $alignment)
	};
}

/// Print formatted text to our console.
///
/// From http://blog.phil-opp.com/rust-os/printing-to-screen.html, but tweaked
/// for HermitCore.
#[macro_export]
macro_rules! print {
	($($arg:tt)+) => ({
		use core::fmt::Write;

		let mut console = crate::console::Console {};
		console.write_fmt(format_args!($($arg)+)).unwrap();
	});
}

/// Print formatted text to our console, followed by a newline.
#[macro_export]
macro_rules! println {
	($($arg:tt)+) => (print!("{}\n", format_args!($($arg)+)));
}

/// Print formatted loader log messages to our console, followed by a newline.
#[macro_export]
macro_rules! loaderlog {
	($($arg:tt)+) => (println!("[LOADER] {}", format_args!($($arg)+)));
}
