// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

pub mod paging;
pub mod processor;
pub mod serial;

use arch::x86_64::paging::{BasePageSize, LargePageSize, PageSize, PageTableEntryFlags};
use arch::x86_64::serial::SerialPort;
use core::{mem, slice, intrinsics};
use elf::*;
use multiboot::Multiboot;
use physicalmem;

extern "C" {
	static mb_info: usize;
}

// CONSTANTS
pub const ELF_ARCH: u16 = ELF_EM_X86_64;

const SERIAL_PORT_ADDRESS: u16 = 0x3F8;
const SERIAL_PORT_BAUDRATE: u32 = 115200;

#[repr(C)]
struct BootInfo {
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
	hcmask: [u8; 4],
}

// VARIABLES
static COM1: SerialPort = SerialPort::new(SERIAL_PORT_ADDRESS);

fn paddr_to_slice<'a>(p: multiboot::PAddr, sz: usize) -> Option<&'a [u8]> {
	unsafe {
		let ptr = mem::transmute(p);
		Some(slice::from_raw_parts(ptr, sz))
	}
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
	let multiboot = Multiboot::new(mb_info as u64, paddr_to_slice).unwrap();
	let modules_address = multiboot
		.modules()
		.expect("Could not find a memory map in the Multiboot information")
		.next()
		.expect("Could not first map address")
		.start as usize;
	let page_address = align_down!(modules_address, BasePageSize::SIZE);
	paging::map::<BasePageSize>(page_address, page_address, 1, PageTableEntryFlags::empty());

	// Iterate through all modules.
	// Collect the start address of the first module and the highest end address of all modules.
	let modules = multiboot.modules().unwrap();
	let mut found_module = false;
	let mut start_address = 0;
	let mut end_address = 0;

	for m in modules {
		found_module = true;

		if start_address == 0 {
			start_address = m.start as usize;
		}

		if m.end as usize > end_address {
			end_address = m.end as usize;
		}
	}

	loaderlog!(
		"Found module: [0x{:x} - 0x{:x}]",
		start_address,
		end_address
	);

	// Memory after the highest end address is unused and available for the physical memory manager.
	// However, we want to move the HermitCore Application to the next 2 MB boundary.
	// So add this boundary and align up the address to be on the safe side.
	end_address += LargePageSize::SIZE;
	physicalmem::init(align_up!(end_address, LargePageSize::SIZE));

	// Identity-map the ELF header of the first module.
	assert!(
		found_module,
		"Could not find a single module in the Multiboot information"
	);
	assert!(start_address > 0);
	loaderlog!("Found an ELF module at {:#X}", start_address);
	let page_address = align_down!(start_address, BasePageSize::SIZE);
	paging::map::<BasePageSize>(page_address, page_address, 1, PageTableEntryFlags::empty());

	start_address
}

pub unsafe fn move_kernel(
	physical_address: usize,
	virtual_address: usize,
	mem_size: usize,
	file_size: usize,
) -> usize {
	// We want to move the application to realize a identify mapping
	let page_count = align_up!(mem_size, LargePageSize::SIZE) / LargePageSize::SIZE;
	loaderlog!("Use {} large pages for the application.", page_count);

	paging::map::<LargePageSize>(
		virtual_address,
		virtual_address,
		page_count,
		PageTableEntryFlags::WRITABLE,
	);

	for i in (0..align_up!(file_size, BasePageSize::SIZE) / BasePageSize::SIZE).rev() {
		let tmp = 0x2000;
		paging::map::<BasePageSize>(
			tmp,
			align_down!(physical_address, BasePageSize::SIZE) + i * BasePageSize::SIZE,
			1,
			PageTableEntryFlags::WRITABLE,
		);

		for j in 0..BasePageSize::SIZE {
			*((virtual_address + i * BasePageSize::SIZE + j) as *mut u8) =
				*((tmp + j) as *const u8);
		}
	}

	virtual_address
}

pub unsafe fn boot_kernel(
	new_physical_address: usize,
	virtual_address: usize,
	mem_size: usize,
	entry_point: usize,
) {
	let BOOT_INFO = &mut *(virtual_address as *mut BootInfo);
	loaderlog!("Found magic number 0x{:x}", BOOT_INFO.magic_number);

	// Supply the parameters to the HermitCore application.
	intrinsics::volatile_store(&mut BOOT_INFO.base, new_physical_address as u64);
	intrinsics::volatile_store(&mut BOOT_INFO.image_size, mem_size as u64);
	intrinsics::volatile_store(&mut BOOT_INFO.mb_info, mb_info as u64);
	intrinsics::volatile_store(
		&mut BOOT_INFO.current_stack_address,
		(virtual_address + mem::size_of::<BootInfo>()) as u64,
	);
	intrinsics::volatile_store(&mut BOOT_INFO.uartport, SERIAL_PORT_ADDRESS);
	intrinsics::volatile_store(&mut BOOT_INFO.uhyve, 0);

	let multiboot = Multiboot::new(mb_info as u64, paddr_to_slice).unwrap();
	if let Some(cmdline) = multiboot.command_line() {
		let address = cmdline.as_ptr();

		// Identity-map the command line.
		let page_address = align_down!(address as usize, BasePageSize::SIZE);
		paging::map::<BasePageSize>(page_address, page_address, 1, PageTableEntryFlags::empty());

		//let cmdline = multiboot.command_line().unwrap();
		intrinsics::volatile_store(&mut BOOT_INFO.cmdline, address as u64);
		intrinsics::volatile_store(&mut BOOT_INFO.cmdsize, cmdline.len() as u64);
	}

	// Jump to the kernel entry point and provide the Multiboot information to it.
	loaderlog!(
		"Jumping to HermitCore Application Entry Point at {:#X}",
		entry_point
	);
	asm!("jmp *$0" :: "r"(entry_point), "{rdx}"(mb_info) : "memory" : "volatile");
}
