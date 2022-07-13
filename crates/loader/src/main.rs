#![no_std]
#![no_main]
#![feature(asm_const)]
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

// TODO: Migrate to upstream plain implementation
// https://github.com/m4b/goblin/pull/317
fn nhdr_from_bytes(bytes: &[u8]) -> Option<&goblin::elf::note::Nhdr32> {
	if bytes
		.as_ptr()
		.align_offset(core::mem::align_of::<goblin::elf::note::Nhdr32>())
		!= 0
	{
		return None;
	}
	if bytes.len() < core::mem::size_of::<goblin::elf::note::Nhdr32>() {
		return None;
	}
	// SAFETY: We just checked alignment and size
	Some(unsafe { &*(bytes.as_ptr() as *const goblin::elf::note::Nhdr32) })
}
