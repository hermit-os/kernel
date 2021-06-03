// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//                    Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

macro_rules! align_down {
	($value:expr, $alignment:expr) => {
		($value) & !($alignment - 1)
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
        $crate::_print(format_args!($($arg)*));
	});
}

/// Print formatted text to our console, followed by a newline.
#[macro_export]
macro_rules! println {
    () => (print!("\n"));
	($($arg:tt)+) => (print!("{}\n", format_args!($($arg)+)));
}

macro_rules! switch_to_kernel {
	() => {
		crate::arch::irq::disable();
		unsafe {
			let user_stack_pointer;
			// Store the user stack pointer and switch to the kernel stack
			llvm_asm!(
				"mov %rsp, $0; mov $1, %rsp"
				: "=r"(user_stack_pointer) : "r"(get_kernel_stack()) :: "volatile"
			);
			core_scheduler().set_current_user_stack(user_stack_pointer);
		}
		crate::arch::irq::enable();
	}
}

#[cfg(feature = "newlib")]
macro_rules! switch_to_user {
	() => {
		use crate::arch::kernel::percore::*;

		crate::arch::irq::disable();
		let user_stack_pointer = core_scheduler().get_current_user_stack();
		unsafe {
			// Switch to the user stack
			llvm_asm!("mov $0, %rsp" :: "r"(user_stack_pointer) :: "volatile");
		}
		crate::arch::irq::enable();
	}
}

macro_rules! kernel_function {
	($f:ident($($x:tt)*)) => {{
		use crate::arch::kernel::percore::*;

		#[allow(clippy::diverging_sub_expression)]
		#[allow(unused_unsafe)]
		#[allow(unused_variables)]
		#[allow(unreachable_code)]
		unsafe {
			crate::arch::irq::disable();
			let user_stack_pointer;
			// Store the user stack pointer and switch to the kernel stack
			llvm_asm!(
				"mov %rsp, $0; mov $1, %rsp"
				: "=r"(user_stack_pointer)
				: "r"(get_kernel_stack())
				:: "volatile"
			);
			core_scheduler().set_current_user_stack(user_stack_pointer);
			crate::arch::irq::enable();

			let ret = $f($($x)*);

			crate::arch::irq::disable();
			// Switch to the user stack
			llvm_asm!("mov $0, %rsp"
				:: "r"(core_scheduler().get_current_user_stack())
				:: "volatile"
			);
			crate::arch::irq::enable();

			ret
		}
	}};
}
