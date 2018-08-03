// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// MIT License
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
// WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

#![feature(asm)]
#![feature(const_fn)]
#![feature(lang_items)]
#![feature(panic_implementation)]
#![feature(panic_info_message)]
#![feature(specialization)]
#![no_std]

// EXTERNAL CRATES
#[macro_use]
extern crate bitflags;

#[cfg(target_arch = "aarch64")]
extern crate byteorder;

#[cfg(target_arch = "aarch64")]
extern crate hermit_dtb;

#[cfg(target_arch = "x86_64")]
extern crate hermit_multiboot;

#[macro_use]
extern crate lazy_static;

#[allow(unused_extern_crates)]
extern crate rlibc;

#[cfg(target_arch = "x86_64")]
extern crate x86;

// MODULES
#[macro_use]
mod macros;

mod arch;
mod console;
mod elf;
mod physicalmem;
mod runtime_glue;

// IMPORTS
use arch::paging::{BasePageSize, LargePageSize, PageSize};
use core::ptr;
use elf::*;

extern "C" {
	static bss_end: u8;
	static mut bss_start: u8;
}

// FUNCTIONS
unsafe fn sections_init() {
	// Initialize .bss section
	ptr::write_bytes(
		&mut bss_start as *mut u8,
		0,
		&bss_end as *const u8 as usize - &bss_start as *const u8 as usize
	);
}

unsafe fn check_kernel_elf_file(start_address: usize) -> (usize, usize, usize, usize, usize) {
	// Verify that this module is a HermitCore ELF executable.
	let header = & *(start_address as *const ElfHeader);
	assert!(header.ident.magic == ELF_MAGIC);
	assert!(header.ident._class == ELF_CLASS_64);
	assert!(header.ident.data == ELF_DATA_2LSB);
	assert!(header.ident.pad[0] == ELF_PAD_HERMIT);
	assert!(header.ty == ELF_ET_EXEC);
	assert!(header.machine == arch::ELF_ARCH);
	loaderlog!("This is a supported HermitCore Application");

	// Get all necessary information about the ELF executable.
	let mut physical_address = 0;
	let mut virtual_address = 0;
	let mut file_size = 0;
	let mut mem_size = 0;

	for i in 0..header.ph_entry_count {
		let program_header = & *((start_address + header.ph_offset + (i * header.ph_entry_size) as usize) as *const ElfProgramHeader);
		if program_header.ty == ELF_PT_LOAD {
			if physical_address == 0 {
				physical_address = start_address + program_header.offset;
			}

			if virtual_address == 0 {
				virtual_address = program_header.virt_addr;
			}

			file_size = program_header.virt_addr + align_up!(program_header.file_size, BasePageSize::SIZE) - virtual_address;
			mem_size = program_header.virt_addr - virtual_address + program_header.mem_size;
		}
	}

	// Verify the information.
	assert!(physical_address % BasePageSize::SIZE == 0);
	assert!(virtual_address % LargePageSize::SIZE == 0);
	assert!(file_size > 0);
	assert!(mem_size > 0);
	loaderlog!("File Size: {} Bytes", file_size);
	loaderlog!("Mem Size:  {} Bytes", mem_size);

	(physical_address, virtual_address, file_size, mem_size, header.entry)
}

/// Entry Point of the HermitCore Loader
/// (called from entry.asm or entry.S)
#[no_mangle]
pub unsafe extern "C" fn loader_main() {
	sections_init();
	arch::message_output_init();

	loaderlog!("Started");

	let start_address = arch::find_kernel();
	let (physical_address, virtual_address, file_size, mem_size, entry_point) = check_kernel_elf_file(start_address);
	let new_physical_address = arch::move_kernel(physical_address, virtual_address, file_size);
	arch::boot_kernel(new_physical_address, virtual_address, mem_size, entry_point);
}
