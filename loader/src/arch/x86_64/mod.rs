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

pub mod paging;
pub mod processor;
pub mod serial;

use arch::x86_64::paging::{BasePageSize, LargePageSize, PageSize, PageTableEntryFlags};
use arch::x86_64::serial::SerialPort;
use elf::*;
use hermit_multiboot::Multiboot;
use physicalmem;
use core::{mem,ptr};

extern "C" {
	static mb_info: usize;
}

// CONSTANTS
pub const ELF_ARCH: u16 = ELF_EM_X86_64;

const SERIAL_PORT_ADDRESS: u16 = 0x3F8;
const SERIAL_PORT_BAUDRATE: u32 = 115200;

#[repr(C)]
struct KernelHeader {
	magic_number: u32,
	version: u32,
	base: u64,
	limit: u64,
	image_size: u64,
	current_stack_address: u64,
	current_percore_address: u64,
	host_logical_addr: u64,
	boot_gtod: u64,
	mb_info: u64,
	cmdline: u64,
	cmdsize: u64,
	cpu_freq: u32,
	boot_processor: u32,
	cpu_online: u32,
	possible_cpus: u32,
	current_boot_id: u32,
	uartport: u16,
	single_kernel: u8,
	uhyve: u8,
	hcip: [u8; 4],
	hcgateway: [u8; 4],
	hcmask: [u8; 4]
}

// VARIABLES
static COM1: SerialPort = SerialPort::new(SERIAL_PORT_ADDRESS);

lazy_static! {
	static ref MULTIBOOT: Multiboot = unsafe { Multiboot::new(mb_info) };
}

// FUNCTIONS
pub fn message_output_init() {
	COM1.init(SERIAL_PORT_BAUDRATE);
}

pub fn output_message_byte(byte: u8) {
	COM1.write_byte(byte);
}

pub unsafe fn find_kernel() -> usize {
	// Identity-map the Multiboot information.
	assert!(mb_info > 0, "Could not find Multiboot information");
	loaderlog!("Found Multiboot information at {:#X}", mb_info);
	let page_address = align_down!(mb_info, BasePageSize::SIZE);
	paging::map::<BasePageSize>(page_address, page_address, 1, PageTableEntryFlags::empty());

	// Load the Multiboot information and identity-map the modules information.
	let modules_address = MULTIBOOT.modules_address().expect("Could not find module information in the Multiboot information");
	let page_address = align_down!(modules_address, BasePageSize::SIZE);
	paging::map::<BasePageSize>(page_address, page_address, 1, PageTableEntryFlags::empty());

	// Iterate through all modules.
	// Collect the start address of the first module and the highest end address of all modules.
	let modules = MULTIBOOT.modules().unwrap();
	let mut found_module = false;
	let mut start_address = 0;
	let mut end_address = 0;

	for m in modules {
		found_module = true;

		if start_address == 0 {
			start_address = m.start_address();
		}

		if m.end_address() > end_address {
			end_address = m.end_address();
		}
	}

	loaderlog!("Found module: [0x{:x} - 0x{:x}]", start_address, end_address);

	// Memory after the highest end address is unused and available for the physical memory manager.
	// However, we want to move the HermitCore Application to the next 2 MB boundary.
	// So add this boundary and align up the address to be on the safe side.
	end_address += LargePageSize::SIZE;
	physicalmem::init(align_up!(end_address, LargePageSize::SIZE));

	// Identity-map the ELF header of the first module.
	assert!(found_module, "Could not find a single module in the Multiboot information");
	assert!(start_address > 0);
	loaderlog!("Found an ELF module at {:#X}", start_address);
	let page_address = align_down!(start_address, BasePageSize::SIZE);
	paging::map::<BasePageSize>(page_address, page_address, 1, PageTableEntryFlags::empty());

	start_address
}

pub unsafe fn move_kernel(physical_address: usize, virtual_address: usize, file_size: usize) -> usize {
	// We want to move the application to realize a identify mapping
	let page_count = (file_size / LargePageSize::SIZE) + 1;
	paging::map::<LargePageSize>(virtual_address, virtual_address, page_count, PageTableEntryFlags::WRITABLE);

	for i in (0..align_up!(file_size, BasePageSize::SIZE)/BasePageSize::SIZE).rev() {
		let tmp = 0x2000;
		paging::map::<BasePageSize>(tmp, align_down!(physical_address, BasePageSize::SIZE)+i*BasePageSize::SIZE,
			1, PageTableEntryFlags::WRITABLE);

		for j in 0..BasePageSize::SIZE {
			*((virtual_address + i*BasePageSize::SIZE + j) as *mut u8) = *((tmp + j) as *const u8);
		}
	}

	virtual_address
}

pub unsafe fn boot_kernel(new_physical_address: usize, virtual_address: usize, mem_size: usize, entry_point: usize) {
	let kernel_header = &mut *(virtual_address as *mut KernelHeader);
	loaderlog!("Found magic number 0x{:x}", kernel_header.magic_number);

	// Supply the parameters to the HermitCore application.
	ptr::write_volatile(&mut kernel_header.base, new_physical_address as u64);
	ptr::write_volatile(&mut kernel_header.image_size, mem_size as u64);
	ptr::write_volatile(&mut kernel_header.mb_info, mb_info as u64);
	ptr::write_volatile(&mut kernel_header.current_stack_address, (virtual_address+mem::size_of::<KernelHeader>()) as u64);
	ptr::write_volatile(&mut kernel_header.uartport, SERIAL_PORT_ADDRESS);

	if let Some(address) = MULTIBOOT.command_line_address() {
		// Identity-map the command line.
		let page_address = align_down!(address, BasePageSize::SIZE);
		paging::map::<BasePageSize>(page_address, page_address, 1, PageTableEntryFlags::empty());

		let cmdline = MULTIBOOT.command_line().unwrap();
		ptr::write_volatile(&mut kernel_header.cmdline, address as u64);
		ptr::write_volatile(&mut kernel_header.cmdsize, cmdline.len() as u64);
	}

	// Jump to the kernel entry point and provide the Multiboot information to it.
	loaderlog!("Jumping to HermitCore Application Entry Point at {:#X}", entry_point);
	asm!("jmp *$0" :: "r"(entry_point), "{rdx}"(mb_info) : "memory" : "volatile");
}
