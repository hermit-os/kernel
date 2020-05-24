// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

pub mod bootinfo;
pub mod paging;
pub mod processor;
pub mod serial;

pub use self::bootinfo::*;
use crate::arch::x86_64::paging::{BasePageSize, LargePageSize, PageSize, PageTableEntryFlags};
use crate::arch::x86_64::serial::SerialPort;
use crate::elf::*;
use crate::physicalmem;
use core::{mem, slice};
use multiboot::Multiboot;

extern "C" {
	static mb_info: usize;
}

// CONSTANTS
pub const ELF_ARCH: u16 = ELF_EM_X86_64;

const KERNEL_STACK_SIZE: usize = 32_768;
const SERIAL_PORT_ADDRESS: u16 = 0x3F8;
const SERIAL_PORT_BAUDRATE: u32 = 115200;

// VARIABLES
static COM1: SerialPort = SerialPort::new(SERIAL_PORT_ADDRESS);
pub static mut BOOT_INFO: BootInfo = BootInfo::new();

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
		.expect("Could not find first map address")
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

	// clear rest of the kernel
	let start = file_size;
	let end = mem_size;
	loaderlog!(
		"Clear BSS from 0x{:x} to 0x{:x}",
		virtual_address + start,
		virtual_address + end
	);
	for i in start..end {
		*((virtual_address + i) as *mut u8) = 0;
	}

	virtual_address
}

pub unsafe fn boot_kernel(
	new_physical_address: usize,
	virtual_address: usize,
	mem_size: usize,
	entry_point: usize,
) {
	// Supply the parameters to the HermitCore application.
	BOOT_INFO.base = new_physical_address as u64;
	BOOT_INFO.image_size = mem_size as u64;
	BOOT_INFO.mb_info = mb_info as u64;
	BOOT_INFO.current_stack_address = (virtual_address - KERNEL_STACK_SIZE) as u64;

	// map stack in the address space
	paging::map::<BasePageSize>(
		virtual_address - KERNEL_STACK_SIZE,
		virtual_address - KERNEL_STACK_SIZE,
		KERNEL_STACK_SIZE / BasePageSize::SIZE,
		PageTableEntryFlags::WRITABLE,
	);

	loaderlog!("BootInfo located at 0x{:x}", &BOOT_INFO as *const _ as u64);
	loaderlog!("Use stack address 0x{:x}", BOOT_INFO.current_stack_address);

	let multiboot = Multiboot::new(mb_info as u64, paddr_to_slice).unwrap();
	if let Some(cmdline) = multiboot.command_line() {
		let address = cmdline.as_ptr();

		// Identity-map the command line.
		let page_address = align_down!(address as usize, BasePageSize::SIZE);
		paging::map::<BasePageSize>(page_address, page_address, 1, PageTableEntryFlags::empty());

		//let cmdline = multiboot.command_line().unwrap();
		BOOT_INFO.cmdline = address as u64;
		BOOT_INFO.cmdsize = cmdline.len() as u64;
	}

	// Jump to the kernel entry point and provide the Multiboot information to it.
	loaderlog!(
		"Jumping to HermitCore Application Entry Point at {:#X}",
		entry_point
	);
	llvm_asm!("jmp *$0" :: "r"(entry_point), "{rdi}"(&BOOT_INFO as *const _ as usize) : "memory" : "volatile");
}
