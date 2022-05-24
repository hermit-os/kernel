#![feature(alloc_error_handler)]
#![cfg_attr(target_arch = "aarch64", feature(asm_const))]
#![allow(incomplete_features)]
#![feature(specialization)]
#![no_std]
#![warn(rust_2018_idioms)]
#![allow(clippy::missing_safety_doc)]

#[macro_use]
extern crate alloc;

#[cfg(target_arch = "x86_64")]
#[macro_use]
extern crate bitflags;

#[macro_use]
pub mod macros;

pub mod arch;
pub mod console;
pub mod kernel;
mod runtime_glue;

use core::{mem::MaybeUninit, ptr::addr_of_mut, slice};

use static_alloc::Bump;

/// A simple bump memory allocator using static storage.
///
/// This allocator is used for bootstrapping.
/// It manages a slice of uninitialized memory and cannot deallocate.
/// The kernel will setup a proper memory allocator later.
#[global_allocator]
static ALLOCATOR: Bump<[u8; 2 * 1024 * 1024]> = Bump::uninit();

pub unsafe fn init_bss() {
	extern "C" {
		static mut bss_start: MaybeUninit<u8>;
		static mut bss_end: MaybeUninit<u8>;
	}

	let start_ptr = addr_of_mut!(bss_start);
	let end_ptr = addr_of_mut!(bss_end);
	let len = end_ptr.offset_from(start_ptr).try_into().unwrap();
	let slice = slice::from_raw_parts_mut(start_ptr, len);
	slice.fill(MaybeUninit::new(0));
}

#[doc(hidden)]
pub fn _print(args: ::core::fmt::Arguments<'_>) {
	use core::fmt::Write;
	unsafe {
		crate::console::CONSOLE.write_fmt(args).unwrap();
	}
}
