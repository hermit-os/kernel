// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//                    Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

#![no_std] // don't link the Rust standard library
#![cfg_attr(not(test), no_main)] // disable all Rust-level entry points
#![cfg_attr(test, allow(dead_code, unused_macros, unused_imports))]

extern crate rusty_loader;

use core::intrinsics::copy;
use rusty_loader::arch;
use rusty_loader::*;

/// Entry Point of the HermitCore Loader
/// (called from entry.asm or entry.S)
#[no_mangle]
pub unsafe extern "C" fn loader_main() -> ! {
	sections_init();
	arch::message_output_init();

	loaderlog!("Started");

	let (start_address, end_address) = arch::find_kernel();
	let (_physical_address, virtual_address, _file_size, mem_size, entry_point) =
		check_kernel_elf_file(start_address);
	let kernel_location = load_kernel(start_address, end_address, mem_size);

	// move kernel to the virtual address
	// TODO: if we have position independent code => moving isn't required
	loaderlog!(
		"Move kernel form 0x{:x} to 0x{:x}",
		kernel_location,
		virtual_address
	);
	arch::map_memory(virtual_address, mem_size);
	copy(
		kernel_location as *const u8,
		virtual_address as *mut u8,
		mem_size,
	);

	arch::boot_kernel(virtual_address, mem_size, entry_point)
}
