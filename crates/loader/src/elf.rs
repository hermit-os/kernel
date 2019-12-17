// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

pub const ELF_MAGIC: u32 = 0x464C_457F;
/// 64-bit file
pub const ELF_CLASS_64: u8 = 0x02;
/// Little-Endian encoding
pub const ELF_DATA_2LSB: u8 = 0x01;
/// HermitCore OSABI identification
pub const ELF_PAD_HERMIT: u8 = 0xFF;

#[repr(C, packed)]
pub struct ElfIdentification {
	pub magic: u32,
	pub _class: u8,
	pub data: u8,
	pub version: u8,
	pub pad: [u8; 8],
	pub nident: u8,
}

/// Executable
pub const ELF_ET_EXEC: u16 = 0x0002;

/// x86_64 architecture
#[allow(dead_code)]
pub const ELF_EM_X86_64: u16 = 0x003E;

/// AArch64 architecture
#[allow(dead_code)]
pub const ELF_EM_AARCH64: u16 = 0x00B7;

#[repr(C, packed)]
pub struct ElfHeader {
	pub ident: ElfIdentification,
	pub ty: u16,
	pub machine: u16,
	pub version: u32,
	pub entry: usize,
	pub ph_offset: usize,
	pub sh_offset: usize,
	pub flags: u32,
	pub header_size: u16,
	pub ph_entry_size: u16,
	pub ph_entry_count: u16,
	pub sh_entry_size: u16,
	pub sh_entry_count: u16,
	pub sh_str_table_index: u16,
}

/// Loadable program segment
pub const ELF_PT_LOAD: u32 = 1;
/// TLS	 segment
pub const ELF_PT_TLS: u32 = 7;

#[repr(C, packed)]
pub struct ElfProgramHeader {
	pub ty: u32,
	pub flags: u32,
	pub offset: usize,
	pub virt_addr: usize,
	pub phys_addr: usize,
	pub file_size: usize,
	pub mem_size: usize,
	pub alignment: usize,
}
