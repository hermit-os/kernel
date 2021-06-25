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
	($f:ident($($x:tt)*)) => {{
		use $crate::arch::{irq, kernel::percore, mm::VirtAddr};

		#[allow(clippy::diverging_sub_expression)]
		#[allow(unused_unsafe)]
		#[allow(unused_variables)]
		#[allow(unreachable_code)]
		unsafe {
			irq::disable();
			let user_stack_pointer;
			// Store the user stack pointer and switch to the kernel stack
			// FIXME: Actually switch stacks https://github.com/hermitcore/libhermit-rs/issues/234
			asm!(
				"mov {}, rsp",
				// "mov rsp, {}",
				out(reg) user_stack_pointer,
				// in(reg) get_kernel_stack(),
				options(nomem, preserves_flags),
			);
			percore::core_scheduler().set_current_user_stack(VirtAddr(user_stack_pointer));
			irq::enable();

			let ret = $f($($x)*);

			irq::disable();
			// Switch to the user stack
			asm!(
				"mov rsp, {}",
				in(reg) percore::core_scheduler().get_current_user_stack().0,
				options(nomem, preserves_flags),
			);
			irq::enable();

			ret
		}
	}};
}
