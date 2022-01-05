#![feature(alloc_error_handler)]
#![cfg_attr(target_arch = "aarch64", feature(asm_const))]
#![allow(incomplete_features)]
#![feature(specialization)]
#![no_std]
#![warn(rust_2018_idioms)]
#![allow(clippy::missing_safety_doc)]

// EXTERNAL CRATES
#[macro_use]
extern crate alloc;

#[cfg(target_arch = "x86_64")]
#[macro_use]
extern crate bitflags;

// MODULES
#[macro_use]
pub mod macros;

pub mod arch;
pub mod console;
pub mod kernel;
pub mod mm;
mod runtime_glue;

use core::ptr;

#[global_allocator]
static ALLOCATOR: mm::allocator::Allocator = mm::allocator::Allocator;

// FUNCTIONS
pub unsafe fn sections_init() {
	extern "C" {
		static bss_end: u8;
		static mut bss_start: u8;
	}

	// Initialize .bss section
	ptr::write_bytes(
		&mut bss_start as *mut u8,
		0,
		&bss_end as *const u8 as usize - &bss_start as *const u8 as usize,
	);
}
