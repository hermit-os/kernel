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

/// Runs `f` on the kernel stack.
///
/// All arguments and return values have to fit into registers:
///
/// ```
/// assert!(mem::size_of::<T>() <= mem::size_of::<usize>());
/// ```
///
/// When working with bigger types, manually route the data over pointers:
///
/// ```
/// f(&arg1, &mut ret);
/// // instead of
/// let ret = f(arg);
/// ```
#[macro_export]
macro_rules! kernel_function {
	($f:ident()) => {
		$crate::arch::switch::kernel_function0($f)
	};

	($f:ident($arg1:expr)) => {
		$crate::arch::switch::kernel_function1($f, $arg1)
	};

	($f:ident($arg1:expr, $arg2:expr)) => {
		$crate::arch::switch::kernel_function2($f, $arg1, $arg2)
	};

	($f:ident($arg1:expr, $arg2:expr, $arg3:expr)) => {
		$crate::arch::switch::kernel_function3($f, $arg1, $arg2, $arg3)
	};

	($f:ident($arg1:expr, $arg2:expr, $arg3:expr, $arg4:expr)) => {
		$crate::arch::switch::kernel_function4($f, $arg1, $arg2, $arg3, $arg4)
	};

	($f:ident($arg1:expr, $arg2:expr, $arg3:expr, $arg4:expr, $arg5:expr)) => {
		$crate::arch::switch::kernel_function5($f, $arg1, $arg2, $arg3, $arg4, $arg5)
	};

	($f:ident($arg1:expr, $arg2:expr, $arg3:expr, $arg4:expr, $arg5:expr, $arg6:expr)) => {
		$crate::arch::switch::kernel_function6($f, $arg1, $arg2, $arg3, $arg4, $arg5, $arg6)
	};
}

#[cfg(target_arch = "aarch64")]
macro_rules! kernel_function {
	($f:ident($($x:tt)*)) => {{
		$f($($x)*)
	}};
}
