// Copyright (c) 2018-2020 Colin Finck, RWTH Aachen University
//                         Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

#![no_std] // don't link the Rust standard library
#![cfg_attr(not(test), no_main)] // disable all Rust-level entry points
#![cfg_attr(test, allow(dead_code, unused_macros, unused_imports))]

extern crate goblin;
extern crate rusty_loader;

use goblin::elf;
use rusty_loader::arch;
use rusty_loader::*;

extern "C" {
	static kernel_end: u8;
	static kernel_start: u8;
}

/// Entry Point of the HermitCore Loader
/// (called from entry.asm or entry.rs)
#[no_mangle]
pub unsafe extern "C" fn loader_main() -> ! {
	sections_init();
	arch::message_output_init();

	loaderlog!(
		"Loader: [0x{:x} - 0x{:x}]",
		&kernel_start as *const u8 as usize,
		&kernel_end as *const u8 as usize
	);

	let app = arch::find_kernel();
	let elf = elf::Elf::parse(&app).expect("Unable to parse ELF file");
	assert_ne!(
		elf.entry, 0,
		"Goblin failed to find entry point of the kernel in the Elf header"
	);
	let mem_size = check_kernel_elf_file(&elf);
	let (kernel_location, entry_point) = load_kernel(&elf, app.as_ptr() as u64, mem_size);

	// boot kernel
	arch::boot_kernel(kernel_location, mem_size, entry_point)
}
