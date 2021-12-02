use crate::arch::{self, get_memory, BOOT_INFO};
use core::ptr::{copy_nonoverlapping, write_bytes};
use goblin::{
	container::{Container, Ctx, Endian},
	elf::{header, program_header, reloc, Dynamic, Elf, Header, ProgramHeader, RelocSection},
	error::{Error as GoblinError, Result as GoblinResult},
};

fn parse_ctx(header: &Header) -> GoblinResult<Ctx> {
	let is_lsb = header.e_ident[header::EI_DATA] == header::ELFDATA2LSB;
	let endianness = Endian::from(is_lsb);
	let class = header.e_ident[header::EI_CLASS];
	if class != header::ELFCLASS64 && class != header::ELFCLASS32 {
		return Err(GoblinError::Malformed(format!(
			"Unknown values in ELF ident header: class: {} endianness: {}",
			class,
			header.e_ident[header::EI_DATA]
		)));
	}
	let is_64 = class == header::ELFCLASS64;
	let container = if is_64 {
		Container::Big
	} else {
		Container::Little
	};
	let ctx = Ctx::new(container, endianness);

	Ok(ctx)
}

pub fn parse(bytes: &[u8]) -> GoblinResult<Elf<'_>> {
	let header = Elf::parse_header(bytes)?;
	let mut elf = Elf::lazy_parse(header)?;
	let ctx = parse_ctx(&header)?;

	elf.program_headers =
		ProgramHeader::parse(bytes, header.e_phoff as usize, header.e_phnum as usize, ctx)?;

	elf.dynamic = Dynamic::parse(bytes, &elf.program_headers, ctx)?;
	if let Some(dynamic) = &elf.dynamic {
		let dyn_info = &dynamic.info;
		elf.dynrelas = RelocSection::parse(bytes, dyn_info.rela, dyn_info.relasz, true, ctx)?;
	}

	Ok(elf)
}

pub fn check_kernel_elf_file(elf: &Elf<'_>) -> u64 {
	if !elf.libraries.is_empty() {
		panic!(
			"Error: file depends on following libraries: {:?}",
			elf.libraries
		);
	}

	// Verify that this module is a HermitCore ELF executable.
	assert!(elf.header.e_type == header::ET_DYN);
	assert!(elf.header.e_machine == arch::ELF_ARCH);
	loaderlog!("This is a supported HermitCore Application");

	// Get all necessary information about the ELF executable.
	let mut file_size: u64 = 0;
	let mut mem_size: u64 = 0;

	for program_header in &elf.program_headers {
		if program_header.p_type == program_header::PT_LOAD {
			file_size = program_header.p_vaddr + program_header.p_filesz;
			mem_size = program_header.p_vaddr + program_header.p_memsz;
		}
	}

	// Verify the information.
	assert!(file_size > 0);
	assert!(mem_size > 0);
	loaderlog!("Found entry point: {:#x}", elf.entry);
	loaderlog!("File Size: {} Bytes", file_size);
	loaderlog!("Mem Size:  {} Bytes", mem_size);

	mem_size
}

pub unsafe fn load_kernel(elf: &Elf<'_>, elf_start: u64, mem_size: u64) -> (u64, u64) {
	loaderlog!("start {:#x}, size {:#x}", elf_start, mem_size);
	if !elf.libraries.is_empty() {
		panic!(
			"Error: file depends on following libraries: {:?}",
			elf.libraries
		);
	}

	// Verify that this module is a HermitCore ELF executable.
	assert!(elf.header.e_type == header::ET_DYN);
	assert!(elf.header.e_machine == arch::ELF_ARCH);

	if elf.header.e_ident[7] != 0xFF {
		loaderlog!("Unsupported OS ABI {:#x}", elf.header.e_ident[7]);
	}

	let address = get_memory(mem_size);
	loaderlog!("Load HermitCore Application at {:#x}", address);

	// load application
	for program_header in &elf.program_headers {
		if program_header.p_type == program_header::PT_LOAD {
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
		} else if program_header.p_type == program_header::PT_TLS {
			BOOT_INFO.tls_start = address + program_header.p_vaddr as u64;
			BOOT_INFO.tls_filesz = program_header.p_filesz as u64;
			BOOT_INFO.tls_memsz = program_header.p_memsz as u64;
			BOOT_INFO.tls_align = program_header.p_align;

			loaderlog!(
				"Found TLS starts at {:#x} (size {} Bytes)",
				BOOT_INFO.tls_start,
				BOOT_INFO.tls_memsz
			);
		}
	}

	// relocate entries (strings, copy-data, etc.) without an addend
	for rel in &elf.dynrels {
		loaderlog!("Unsupported relocation type {}", rel.r_type);
	}

	extern "C" {
		static kernel_end: u8;
	}

	// relocate entries (strings, copy-data, etc.) with an addend
	for rela in &elf.dynrelas {
		match rela.r_type {
			#[cfg(target_arch = "x86_64")]
			reloc::R_X86_64_RELATIVE => {
				use crate::arch::x86_64::paging::{LargePageSize, PageSize};

				let offset = (address + rela.r_offset) as *mut u64;
				let new_addr =
					align_up!(&kernel_end as *const u8 as usize, LargePageSize::SIZE) as u64;
				*offset = (new_addr as i64 + rela.r_addend.unwrap_or(0)) as u64;
			}
			#[cfg(target_arch = "aarch64")]
			reloc::R_AARCH64_RELATIVE => {
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
