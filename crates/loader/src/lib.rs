// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

#![feature(allocator_api)]
#![feature(const_raw_ptr_to_usize_cast)]
#![feature(lang_items)]
#![feature(llvm_asm)]
#![feature(global_asm)]
#![feature(asm)]
#![feature(panic_info_message)]
#![feature(specialization)]
#![feature(naked_functions)]
#![feature(const_raw_ptr_deref)]
#![feature(core_intrinsics)]
#![no_std]

// EXTERNAL CRATES
#[macro_use]
extern crate bitflags;
extern crate goblin;
#[cfg(target_arch = "x86_64")]
extern crate multiboot;
#[cfg(target_arch = "x86_64")]
extern crate x86;

#[cfg(target_arch = "x86_64")]
use crate::arch::x86_64::paging::{LargePageSize, PageSize};
use crate::arch::{get_memory, BOOT_INFO, ELF_ARCH};
use core::convert::TryInto;
use core::intrinsics::{copy_nonoverlapping, write_bytes};
use core::ptr;
use goblin::elf;
use goblin::elf::program_header::{PT_LOAD, PT_TLS};
use goblin::elf64::reloc::*;

// MODULES
#[macro_use]
pub mod macros;

pub mod arch;
pub mod console;
pub mod mm;
mod runtime_glue;

extern "C" {
	#[allow(dead_code)]
	static kernel_end: u8;
	static bss_end: u8;
	static mut bss_start: u8;
}

#[global_allocator]
static ALLOCATOR: &'static mm::allocator::Allocator = &mm::allocator::Allocator;

// FUNCTIONS
pub unsafe fn sections_init() {
	// Initialize .bss section
	ptr::write_bytes(
		&mut bss_start as *mut u8,
		0,
		&bss_end as *const u8 as usize - &bss_start as *const u8 as usize,
	);
}

pub unsafe fn load_kernel(elf: &elf::Elf, elf_start: u64, mem_size: u64) -> (u64, u64) {
	loaderlog!("start 0x{:x}, size 0x{:x}", elf_start, mem_size);
	if elf.libraries.len() > 0 {
		panic!(
			"Error: file depends on following libraries: {:?}",
			elf.libraries
		);
	}

	// Verify that this module is a HermitCore ELF executable.
	assert!(elf.header.e_type == elf::header::ET_DYN);
	assert!(elf.header.e_machine == ELF_ARCH);

	if elf.header.e_ident[7] != 0xFF {
		loaderlog!("Unsupported OS ABI 0x{:x}", elf.header.e_ident[7]);
	}

	let address = get_memory(mem_size);
	loaderlog!("Load HermitCore Application at 0x{:x}", address);

	// load application
	for program_header in &elf.program_headers {
		if program_header.p_type == PT_LOAD {
			let pos = program_header.p_vaddr;

			copy_nonoverlapping(
				(elf_start + program_header.p_offset) as *const u8,
				(address + pos) as *mut u8,
				program_header.p_filesz.try_into().unwrap(),
			);
			write_bytes(
				(address + pos + program_header.p_filesz) as *mut u8,
				0,
				(program_header.p_memsz - program_header.p_filesz)
					.try_into()
					.unwrap(),
			);
		} else if program_header.p_type == PT_TLS {
			BOOT_INFO.tls_start = address + program_header.p_vaddr as u64;
			BOOT_INFO.tls_filesz = program_header.p_filesz as u64;
			BOOT_INFO.tls_memsz = program_header.p_memsz as u64;

			loaderlog!(
				"Found TLS starts at 0x{:x} (size {} Bytes)",
				BOOT_INFO.tls_start,
				BOOT_INFO.tls_memsz
			);
		}
	}

	// relocate entries (strings, copy-data, etc.) without an addend
	for rel in &elf.dynrels {
		loaderlog!("Unsupported relocation type {}", rel.r_type);
	}

	// relocate entries (strings, copy-data, etc.) with an addend
	for rela in &elf.dynrelas {
		match rela.r_type {
			#[cfg(target_arch = "x86_64")]
			R_X86_64_RELATIVE => {
				let offset = (address + rela.r_offset) as *mut u64;
				let new_addr =
					align_up!(&kernel_end as *const u8 as usize, LargePageSize::SIZE) as u64;
				*offset = (new_addr as i64 + rela.r_addend.unwrap_or(0)) as u64;
			}
			#[cfg(target_arch = "aarch64")]
			R_AARCH64_RELATIVE => {
				let offset = (address + rela.r_offset) as *mut u64;
				*offset = (address as i64 + rela.r_addend.unwrap_or(0)) as u64;
			}
			_ => {
				loaderlog!("Unsupported relocation type {}", rela.r_type);
			}
		}
	}

	(address, elf.entry + address)
}

pub fn check_kernel_elf_file(elf: &elf::Elf) -> u64 {
	if elf.libraries.len() > 0 {
		panic!(
			"Error: file depends on following libraries: {:?}",
			elf.libraries
		);
	}

	// Verify that this module is a HermitCore ELF executable.
	assert!(elf.header.e_type == elf::header::ET_DYN);
	assert!(elf.header.e_machine == ELF_ARCH);
	loaderlog!("This is a supported HermitCore Application");

	// Get all necessary information about the ELF executable.
	let mut file_size: u64 = 0;
	let mut mem_size: u64 = 0;

	for program_header in &elf.program_headers {
		if program_header.p_type == PT_LOAD {
			file_size = program_header.p_vaddr + program_header.p_filesz;
			mem_size = program_header.p_vaddr + program_header.p_memsz;
		}
	}

	// Verify the information.
	assert!(file_size > 0);
	assert!(mem_size > 0);
	loaderlog!("Found entry point: 0x{:x}", elf.entry);
	loaderlog!("File Size: {} Bytes", file_size);
	loaderlog!("Mem Size:  {} Bytes", mem_size);

	mem_size
}
