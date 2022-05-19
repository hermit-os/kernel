#![no_std] // don't link the Rust standard library
#![cfg_attr(not(test), no_main)] // disable all Rust-level entry points
#![cfg_attr(test, allow(dead_code, unused_macros, unused_imports))]
#![warn(rust_2018_idioms)]
#![allow(clippy::missing_safety_doc)]

#[macro_use]
extern crate rusty_loader;

use rusty_loader::{arch, sections_init, kernel};

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
		"Loader: [{:#x} - {:#x}]",
		&kernel_start as *const u8 as usize,
		&kernel_end as *const u8 as usize
	);

	let app = arch::find_kernel();
	let elf = kernel::parse(app).expect("Unable to parse ELF file");
	assert_ne!(
		elf.entry, 0,
		"Goblin failed to find entry point of the kernel in the Elf header"
	);
	let mem_size = kernel::check_kernel_elf_file(&elf);
	let (elf_location, kernel_location, entry_point) =
		kernel::load_kernel(&elf, app.as_ptr() as u64, mem_size);

	// boot kernel
	arch::boot_kernel(elf_location, kernel_location, mem_size, entry_point)
}
