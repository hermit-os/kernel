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
#![feature(hint_core_should_pause)]
#![feature(lang_items)]
#![feature(repr_align)]
#![feature(specialization)]
#![no_std]

// EXTERNAL CRATES
#[macro_use]
extern crate bitflags;

extern crate multiboot;

#[allow(unused_extern_crates)]
extern crate rlibc;

extern crate x86;

// MODULES
mod console;
mod elf;

#[macro_use]
mod macros;

mod paging;
mod runtime_glue;
mod serial;

// IMPORTS
use core::{mem, ptr, slice};
use elf::*;
use paging::{BasePageSize, LargePageSize, PageSize, PageTableEntryFlags};
use serial::SerialPort;

extern "C" {
	static bss_end: u8;
	static mut bss_start: u8;
	static mb_info: multiboot::PAddr;
}

// CONSTANTS
const HERMIT_KERNEL_OFFSET_BASE:       usize = 0x08;
const HERMIT_KERNEL_OFFSET_IMAGE_SIZE: usize = 0x38;
const HERMIT_KERNEL_OFFSET_CMDLINE:    usize = 0xA0;
const HERMIT_KERNEL_OFFSET_CMDSIZE:    usize = 0xA8;

const SERIAL_PORT_ADDRESS: u16 = 0x3F8;
const SERIAL_PORT_BAUDRATE: u32 = 115200;

// VARIABLES
static COM1: SerialPort = SerialPort::new(SERIAL_PORT_ADDRESS);

// FUNCTIONS
fn message_output_init() {
	COM1.init(SERIAL_PORT_BAUDRATE);
}

pub fn output_message_byte(byte: u8) {
	COM1.write_byte(byte);
}

fn paddr_to_slice<'a>(p: multiboot::PAddr, sz: usize) -> Option<&'a [u8]> {
	unsafe {
		let ptr = mem::transmute(p);
		Some(slice::from_raw_parts(ptr, sz))
	}
}

unsafe fn sections_init() {
	// Initialize .bss section
	ptr::write_bytes(
		&mut bss_start as *mut u8,
		0,
		&bss_end as *const u8 as usize - &bss_start as *const u8 as usize
	);
}

/// Entry Point of the HermitCore Loader
/// (called from entry.asm)
#[no_mangle]
pub unsafe extern "C" fn loader_main() {
	sections_init();
	message_output_init();

	loaderlog!("Started");

	assert!(mb_info > 0, "Got no Multiboot information");
	paging::init();

	// Parse the Multiboot info and find the first module.
	let mb = multiboot::Multiboot::new(mb_info, paddr_to_slice).unwrap();
	let app = mb.modules().expect("Could not find any modules in Multiboot info")
		.next().expect("Could not access the first module");
	assert!(app.start > 0);

	// Verify that this module is a HermitCore ELF executable.
	let header = & *(app.start as *const ElfHeader);
	assert!(header.ident.magic == ELF_MAGIC);
	assert!(header.ident._class == ELF_CLASS_64);
	assert!(header.ident.data == ELF_DATA_2LSB);
	assert!(header.ident.pad[0] == ELF_PAD_HERMIT);
	assert!(header.ty == ELF_ET_EXEC);
	assert!(header.machine == ELF_EM_X86_64);
	loaderlog!("Found a HermitCore Application ELF file at {:#X}", app.start);

	// Get all necessary information about the ELF executable.
	let mut physical_address = 0;
	let mut virtual_address = 0;
	let mut file_size = 0;
	let mut mem_size = 0;

	for i in 0..header.ph_entry_count {
		let program_header = & *((app.start as usize + header.ph_offset + (i * header.ph_entry_size) as usize) as *const ElfProgramHeader);
		if program_header.ty == ELF_PT_LOAD {
			if physical_address == 0 {
				physical_address = app.start as usize + program_header.offset;
			}

			if virtual_address == 0 {
				virtual_address = program_header.virt_addr;
			}

			file_size = program_header.virt_addr + align_up!(program_header.file_size, BasePageSize::SIZE) - virtual_address;
			mem_size += program_header.mem_size;
		}
	}

	// Verify the information.
	assert!(physical_address & (BasePageSize::SIZE - 1) == 0);
	assert!(virtual_address & (LargePageSize::SIZE - 1) == 0);
	assert!(file_size > 0);
	assert!(mem_size > 0);
	loaderlog!("File Size: {} Bytes", file_size);
	loaderlog!("Mem Size:  {} Bytes", mem_size);

	// We want to move the application to the next 2 MB boundary to let the HermitCore kernel map it as a large page.
	// First calculate the displacement and map a range large enough to span the original code and its new location.
	let new_physical_address = align_up!(physical_address, LargePageSize::SIZE);
	let displacement = new_physical_address - physical_address;
	let page_count = (file_size + displacement) / BasePageSize::SIZE;
	paging::map::<BasePageSize>(virtual_address, physical_address, page_count, PageTableEntryFlags::WRITABLE);

	// Supply the parameters to the HermitCore application.
	*((virtual_address + HERMIT_KERNEL_OFFSET_BASE) as *mut usize) = new_physical_address;
	*((virtual_address + HERMIT_KERNEL_OFFSET_IMAGE_SIZE) as *mut usize) = mem_size;

	if let Some(cmdline) = mb.command_line() {
		*((virtual_address + HERMIT_KERNEL_OFFSET_CMDLINE) as *mut usize) = cmdline.as_ptr() as usize;
		*((virtual_address + HERMIT_KERNEL_OFFSET_CMDSIZE) as *mut usize) = cmdline.len();
	}

	// Now copy the code byte-wise to the new region, starting from the upper bytes.
	for i in (0..file_size).rev() {
		*((virtual_address + displacement + i) as *mut u8) = *((virtual_address + i) as *const u8);
	}

	// Remap the virtual address to the new physical address, now as a large page.
	let page_count = file_size / LargePageSize::SIZE;
	paging::map::<LargePageSize>(virtual_address, new_physical_address, page_count, PageTableEntryFlags::WRITABLE);

	// Jump to the kernel entry point and provide the Multiboot information to it.
	loaderlog!("Jumping to HermitCore Application Entry Point at {:#X}", header.entry);
	asm!("jmp *$0" :: "r"(header.entry), "{rdx}"(mb_info) : "memory" : "volatile");
}
