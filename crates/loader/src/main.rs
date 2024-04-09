#![no_std]
#![no_main]
#![warn(rust_2018_idioms)]
#![warn(unsafe_op_in_unsafe_fn)]
#![allow(unstable_name_collisions)]
#![allow(clippy::missing_safety_doc)]

#[macro_use]
mod macros;

#[cfg(target_os = "none")]
mod allocator;
mod arch;
mod console;
#[cfg(target_os = "none")]
mod log;
mod os;

#[cfg(any(
	target_os = "uefi",
	all(target_arch = "x86_64", target_os = "none", not(feature = "fc"))
))]
extern crate alloc;

#[doc(hidden)]
fn _print(args: core::fmt::Arguments<'_>) {
	use core::fmt::Write;

	console::CONSOLE.lock().write_fmt(args).unwrap();
}
