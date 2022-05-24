#![no_std] // don't link the Rust standard library
#![cfg_attr(not(test), no_main)] // disable all Rust-level entry points
#![cfg_attr(test, allow(dead_code, unused_macros, unused_imports))]
#![warn(rust_2018_idioms)]
#![allow(clippy::missing_safety_doc)]

#[macro_use]
extern crate rusty_loader;

use core::{mem::MaybeUninit, slice};

use rusty_loader::{
	arch::{self, BOOT_INFO},
	init_bss,
	kernel::{LoadInfo, Object},
};

extern "C" {
	static kernel_end: u8;
	static kernel_start: u8;
}

/// Entry Point of the HermitCore Loader
/// (called from entry.asm or entry.rs)
#[no_mangle]
pub unsafe extern "C" fn loader_main() -> ! {
	init_bss();
	arch::message_output_init();

	loaderlog!(
		"Loader: [{:#x} - {:#x}]",
		&kernel_start as *const u8 as usize,
		&kernel_end as *const u8 as usize
	);

	let kernel = Object::parse(arch::find_kernel());

	let memory = {
		let mem_size = kernel.mem_size();
		let kernel_addr = arch::get_memory(mem_size as u64);
		slice::from_raw_parts_mut(kernel_addr as *mut MaybeUninit<u8>, mem_size)
	};

	let LoadInfo {
		elf_location,
		entry_point,
		tls_info,
	} = kernel.load_kernel(memory);

	if let Some(tls_info) = tls_info {
		tls_info.insert_into(&mut BOOT_INFO);
	}

	arch::boot_kernel(
		elf_location,
		memory.as_ptr() as u64,
		memory.len() as u64,
		entry_point,
	)
}
