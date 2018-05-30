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

#![no_std]

// EXTERNAL CRATES
#[macro_use]
extern crate bitflags;

// IMPORTS
use core::{slice, str};


bitflags! {
	// See Multiboot Specification version 0.6.96
	struct Flags: u32 {
		const MULTIBOOT_INFO_MEMORY           = 0x00000001;
		const MULTIBOOT_INFO_BOOTDEV          = 0x00000002;
		const MULTIBOOT_INFO_CMDLINE          = 0x00000004;
		const MULTIBOOT_INFO_MODS             = 0x00000008;
		const MULTIBOOT_INFO_AOUT_SYMS        = 0x00000010;
		const MULTIBOOT_INFO_ELF_SHDR         = 0x00000020;
		const MULTIBOOT_INFO_MEM_MAP          = 0x00000040;
		const MULTIBOOT_INFO_DRIVE_INFO       = 0x00000080;
		const MULTIBOOT_INFO_CONFIG_TABLE     = 0x00000100;
		const MULTIBOOT_INFO_BOOT_LOADER_NAME = 0x00000200;
		const MULTIBOOT_INFO_APM_TABLE        = 0x00000400;
		const MULTIBOOT_INFO_VBE_INFO         = 0x00000800;
		const MULTIBOOT_INFO_FRAMEBUFFER_INFO = 0x00001000;
	}
}

#[repr(C, packed)]
struct MultibootHeader {
	flags: Flags,
	mem_lower: u32,
	mem_upper: u32,
	boot_device: u32,
	cmdline: u32,
	mods_count: u32,
	mods_addr: u32,
	elf_symbols: [u32; 4],
	mmap_length: u32,
	mmap_addr: u32,
	drives_length: u32,
	drives_addr: u32,
	config_table: u32,
	boot_loader_name: u32,
	apm_table: u32,
	vbe_control_info: u32,
	vbe_mode_info: u32,
	vbe_mode: u16,
	vbe_interface_off: u16,
	vbe_interface_len: u16,
	framebuffer_addr: u64,
	framebuffer_pitch: u32,
	framebuffer_width: u32,
	framebuffer_height: u32,
	framebuffer_bpp: u8,
	framebuffer_type: u8,
	color_info: [u8; 6],
}

pub struct Multiboot {
	header: &'static MultibootHeader,
}

impl Multiboot {
	pub unsafe fn new(address: usize) -> Self {
		Self { header: & *(address as *const MultibootHeader) }
	}

	pub fn command_line_address(&self) -> Option<usize> {
		if {self.header.flags}.contains(Flags::MULTIBOOT_INFO_CMDLINE) {
			Some(self.header.cmdline as usize)
		} else {
			None
		}
	}

	pub unsafe fn command_line(&self) -> Option<&'static str> {
		self.command_line_address().map(|address| {
			let mut count = 0;
			while *((address + count) as *const u8) != 0 {
				count += 1;
			}

			let slice = slice::from_raw_parts(address as *const u8, count);
			str::from_utf8_unchecked(slice)
		})
	}

	pub fn modules_address(&self) -> Option<usize> {
		if {self.header.flags}.contains(Flags::MULTIBOOT_INFO_MODS) {
			Some(self.header.mods_addr as usize)
		} else {
			None
		}
	}

	pub unsafe fn modules(&self) -> Option<&'static [Module]> {
		self.modules_address().map(|address| {
			let ptr = address as *const Module;
			slice::from_raw_parts(ptr, self.header.mods_count as usize)
		})
	}

	pub fn memory_map_address(&self) -> Option<usize> {
		if {self.header.flags}.contains(Flags::MULTIBOOT_INFO_MEM_MAP) {
			Some(self.header.mmap_addr as usize)
		} else {
			None
		}
	}

	pub fn memory_map(&self) -> Option<MemoryMapIter> {
		self.memory_map_address().map(|address|
			MemoryMapIter {
				current: address,
				end: address + self.header.mmap_length as usize,
			}
		)
	}
}

#[repr(C, packed)]
pub struct Module {
	mod_start: u32,
	mod_end: u32,
	string: u32,
	reserved: u32,
}

impl Module {
	#[inline]
	pub fn start_address(&self) -> usize {
		self.mod_start as usize
	}

	#[inline]
	pub fn end_address(&self) -> usize {
		self.mod_end as usize
	}
}


const MEMORY_TYPE_AVAILABLE_RAM: u32 = 1;

#[repr(C, packed)]
pub struct MemoryMapEntry {
	size: u32,
	base_addr: u64,
	length: u64,
	ty: u32
}

impl MemoryMapEntry {
	#[inline]
	pub fn base_address(&self) -> usize {
		self.base_addr as usize
	}

	#[inline]
	pub fn is_available(&self) -> bool {
		self.ty == MEMORY_TYPE_AVAILABLE_RAM
	}

	#[inline]
	pub fn length(&self) -> usize {
		self.length as usize
	}
}

pub struct MemoryMapIter {
	current: usize,
	end: usize,
}

impl Iterator for MemoryMapIter {
	type Item = &'static MemoryMapEntry;

	fn next(&mut self) -> Option<&'static MemoryMapEntry> {
		if self.current < self.end {
			let entry = unsafe { & *(self.current as *const MemoryMapEntry) };
			self.current += entry.size as usize + 4;
			Some(entry)
		} else {
			None
		}
	}
}
