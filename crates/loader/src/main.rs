#![no_std]
#![no_main]
#![warn(rust_2018_idioms)]
#![cfg_attr(target_arch = "riscv64", allow(unstable_name_collisions))]
#![allow(clippy::missing_safety_doc)]
#![allow(unstable_name_collisions)]

#[macro_use]
mod macros;

mod arch;
mod console;
mod log;
#[cfg(target_os = "none")]
mod none;
#[cfg(target_os = "uefi")]
mod uefi;

use core::fmt::{self, Write};

#[doc(hidden)]
fn _print(args: fmt::Arguments<'_>) {
	unsafe {
		console::CONSOLE.write_fmt(args).unwrap();
	}
}
