//! PVH boot.
//!
//! Adapted from the pvh crate examples.

mod gdt;
mod page_tables;
mod stack;

use self::stack::{STACK, Stack};
use crate::env;

/// The PVH entry point.
#[allow(bad_asm_style)]
#[unsafe(naked)]
pub(crate) unsafe extern "C" fn pvh_start32() -> ! {
	core::arch::naked_asm!(
		".code32",
		include_str!("movgot32.s"),
		include_str!("pvh_start32.s"),
		".code64",

		level_4_table = sym page_tables::LEVEL_4_TABLE,
		gdt_ptr = sym gdt::GDT_PTR,
		kernel_data_selector = const gdt::Gdt::kernel_data_selector().0,

		stack = sym STACK,
		stack_size = const size_of::<Stack>(),
		kernel_code_selector = const gdt::Gdt::kernel_code_selector().0,
		rust_start = sym rust_start,
	);
}

pvh::xen_elfnote_phys32_entry!(pvh_start32);

/// The native ELF entry point.
#[unsafe(no_mangle)]
#[unsafe(naked)]
unsafe extern "C" fn _start() -> ! {
	core::arch::naked_asm!("2: jmp 2b");
}

/// The Rust entry point.
unsafe extern "C" fn rust_start(start_info_paddr: u32) -> ! {
	debug!("Entered Rust.");

	unsafe {
		env::set_start_info_paddr(start_info_paddr);
	}

	crate::rt::boot_processor_main()
}
