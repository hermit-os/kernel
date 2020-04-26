// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

#![feature(const_fn)]
#![feature(lang_items)]
#![feature(llvm_asm)]
#![feature(panic_info_message)]
#![feature(specialization)]
#![feature(naked_functions)]
#![feature(const_raw_ptr_deref)]
#![feature(core_intrinsics)]
#![no_std]

// EXTERNAL CRATES
#[macro_use]
extern crate bitflags;

#[cfg(target_arch = "x86_64")]
extern crate multiboot;

#[cfg(target_arch = "x86_64")]
extern crate x86;

// MODULES
#[macro_use]
pub mod macros;

pub mod arch;
pub mod console;
mod elf;
mod physicalmem;
mod runtime_glue;

// IMPORTS
use arch::paging::{BasePageSize, LargePageSize, PageSize};
use arch::BOOT_INFO;
use core::ptr;
use elf::*;

extern "C" {
	static bss_end: u8;
	static mut bss_start: u8;
}

// FUNCTIONS
pub unsafe fn sections_init() {
	// Initialize .bss section
	ptr::write_bytes(
		&mut bss_start as *mut u8,
		0,
		&bss_end as *const u8 as usize - &bss_start as *const u8 as usize,
	);
}

pub unsafe fn check_kernel_elf_file(start_address: usize) -> (usize, usize, usize, usize, usize) {
	// Verify that this module is a HermitCore ELF executable.
	let header = &*(start_address as *const ElfHeader);
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
		let program_header = &*((start_address
			+ header.ph_offset
			+ (i * header.ph_entry_size) as usize) as *const ElfProgramHeader);
		if program_header.ty == ELF_PT_LOAD {
			if physical_address == 0 {
				physical_address = start_address + program_header.offset;
			}

			if virtual_address == 0 {
				virtual_address = program_header.virt_addr;
			}

			file_size = program_header.virt_addr + program_header.file_size - virtual_address;
			mem_size = program_header.virt_addr + program_header.mem_size - virtual_address;
		} else if program_header.ty == ELF_PT_TLS {
			BOOT_INFO.tls_start = program_header.virt_addr as u64;
			BOOT_INFO.tls_filesz = program_header.file_size as u64;
			BOOT_INFO.tls_memsz = program_header.mem_size as u64;

			loaderlog!(
				"Found TLS starts at 0x{:x} (size {} Bytes)",
				BOOT_INFO.tls_start,
				BOOT_INFO.tls_memsz
			);
		}
	}

	// Verify the information.
	assert!(physical_address % BasePageSize::SIZE == 0);
	assert!(virtual_address % LargePageSize::SIZE == 0);
	assert!(file_size > 0);
	assert!(mem_size > 0);
	loaderlog!("File Size: {} Bytes", file_size);
	loaderlog!("Mem Size:  {} Bytes", mem_size);

	(
		physical_address,
		virtual_address,
		file_size,
		mem_size,
		header.entry,
	)
}
