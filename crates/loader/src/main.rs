#![no_std]
#![no_main]
#![feature(asm_const)]
#![feature(maybe_uninit_write_slice)]
#![feature(specialization)]
#![warn(rust_2018_idioms)]
#![allow(incomplete_features)]
#![allow(clippy::missing_safety_doc)]

#[macro_use]
mod macros;

mod arch;
mod console;
mod kernel;

use core::{
	fmt::{self, Write},
	mem::MaybeUninit,
	ptr::addr_of_mut,
	slice,
};

// Workaround for https://github.com/hermitcore/rusty-loader/issues/117
use rusty_loader as _;

use arch::BOOT_INFO;
use kernel::{LoadInfo, Object};

extern "C" {
	static kernel_end: u8;
	static kernel_start: u8;
}

/// Entry Point of the HermitCore Loader
/// (called from entry.asm or entry.rs)
#[no_mangle]
unsafe extern "C" fn loader_main() -> ! {
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

unsafe fn init_bss() {
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

#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
	// We can't use `println!` or related macros, because `_print` unwraps a result and might panic again
	writeln!(unsafe { &mut console::CONSOLE }, "[LOADER] {info}").ok();

	loop {}
}

#[doc(hidden)]
fn _print(args: fmt::Arguments<'_>) {
	unsafe {
		console::CONSOLE.write_fmt(args).unwrap();
	}
}
