#![no_std]
#![no_main]
#![feature(maybe_uninit_write_slice)]
#![feature(specialization)]
#![warn(rust_2018_idioms)]
#![allow(incomplete_features)]
#![allow(clippy::missing_safety_doc)]
#![cfg_attr(target_os = "uefi", feature(abi_efiapi))]

#[macro_use]
mod macros;

mod arch;
mod console;
mod kernel;
mod log;
#[cfg(target_os = "none")]
mod none;
#[cfg(target_os = "uefi")]
mod uefi;

use core::fmt::{self, Write};

// Workaround for https://github.com/hermitcore/rusty-loader/issues/117
use rusty_loader as _;

#[doc(hidden)]
fn _print(args: fmt::Arguments<'_>) {
	unsafe {
		console::CONSOLE.write_fmt(args).unwrap();
	}
}
