use crate::arch;
use crate::console;
use crate::kernel::{LoadInfo, Object};

use core::{fmt::Write, mem::MaybeUninit, ptr::addr_of_mut, slice};

extern "C" {
	static kernel_end: u8;
	static kernel_start: u8;
}

/// Entry Point of the BIOS Loader
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

	arch::boot_kernel(
		tls_info,
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
