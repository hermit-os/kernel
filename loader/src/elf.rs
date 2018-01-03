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

pub const ELF_MAGIC: u32     = 0x464C_457F;
pub const ELF_CLASS_64: u8   = 0x02;      /// 64-bit file
pub const ELF_DATA_2LSB: u8  = 0x01;      /// Little-Endian encoding
pub const ELF_PAD_HERMIT: u8 = 0x42;      /// HermitCore OSABI identification

#[repr(C, packed)]
pub struct ElfIdentification {
	pub magic: u32,
	pub _class: u8,
	pub data: u8,
	pub version: u8,
	pub pad: [u8; 8],
	pub nident: u8
}


pub const ELF_ET_EXEC: u16   = 0x0002;    /// Executable
pub const ELF_EM_X86_64: u16 = 0x003E;    /// x86_64 architecture

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


pub const ELF_PT_LOAD: u32 = 1;    /// Loadable program segment

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
