pub mod paging;
pub mod physicalmem;

#[cfg(target_os = "none")]
use core::ptr::write_bytes;
#[cfg(target_os = "none")]
use core::{cmp, mem, slice};

use hermit_entry::{
	boot_info::{BootInfo, HardwareInfo, PlatformInfo, RawBootInfo, SerialPortBase},
	elf::LoadedKernel,
	Entry,
};
use log::info;
#[cfg(target_os = "none")]
use multiboot::information::{MemoryManagement, Multiboot, PAddr};
use uart_16550::SerialPort;

use paging::{BasePageSize, LargePageSize, PageSize, PageTableEntryFlags};

#[cfg(target_os = "none")]
extern "C" {
	static mb_info: usize;
	static kernel_end: u8;
}

// CONSTANTS
const KERNEL_STACK_SIZE: u64 = 32_768;
const SERIAL_IO_PORT: u16 = 0x3F8;

// VARIABLES
static mut COM1: SerialPort = unsafe { SerialPort::new(SERIAL_IO_PORT) };

#[cfg(target_os = "none")]
struct Mem;
#[cfg(target_os = "none")]
static mut MEM: Mem = Mem;

#[cfg(target_os = "none")]
impl MemoryManagement for Mem {
	unsafe fn paddr_to_slice<'a>(&self, p: PAddr, sz: usize) -> Option<&'static [u8]> {
		let ptr = p as *const u8;
		Some(slice::from_raw_parts(ptr, sz))
	}

	// If you only want to read fields, you can simply return `None`.
	unsafe fn allocate(&mut self, _length: usize) -> Option<(PAddr, &mut [u8])> {
		None
	}

	unsafe fn deallocate(&mut self, addr: PAddr) {
		if addr != 0 {
			unimplemented!()
		}
	}
}

// FUNCTIONS
pub fn message_output_init() {
	unsafe { COM1.init() };
}

pub fn output_message_byte(byte: u8) {
	unsafe { COM1.send(byte) };
}

#[cfg(target_os = "uefi")]
pub unsafe fn find_kernel() -> &'static [u8] {
	&[1, 2, 3]
}

#[cfg(target_os = "uefi")]
pub unsafe fn boot_kernel(
	_elf_address: Option<u64>,
	_virtual_address: u64,
	_mem_size: u64,
	_entry_point: u64,
) -> ! {
	loop {}
}

#[cfg(target_os = "none")]
pub unsafe fn find_kernel() -> &'static [u8] {
	// Identity-map the Multiboot information.
	assert!(mb_info > 0, "Could not find Multiboot information");
	info!("Found Multiboot information at {:#x}", mb_info);
	let page_address = align_down!(mb_info, BasePageSize::SIZE);
	paging::map::<BasePageSize>(page_address, page_address, 1, PageTableEntryFlags::WRITABLE);

	// Load the Multiboot information and identity-map the modules information.
	let multiboot = Multiboot::from_ptr(mb_info as u64, &mut MEM).unwrap();
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

	info!("Found module: [{:#x} - {:#x}]", start_address, end_address);
	let elf_start = start_address;
	let elf_len = end_address - start_address;
	info!("Module length: {:#x}", elf_len);

	let free_memory_address = align_up!(end_address, LargePageSize::SIZE);
	// TODO: Workaround for https://github.com/hermitcore/rusty-loader/issues/96
	let free_memory_address = cmp::max(free_memory_address, 0x800000);
	// Memory after the highest end address is unused and available for the physical memory manager.
	physicalmem::init(free_memory_address);

	// Identity-map the ELF header of the first module.
	assert!(
		found_module,
		"Could not find a single module in the Multiboot information"
	);
	assert!(start_address > 0);
	info!("Found an ELF module at {:#x}", start_address);
	let page_address = align_down!(start_address, BasePageSize::SIZE);
	let counter =
		(align_up!(start_address, LargePageSize::SIZE) - page_address) / BasePageSize::SIZE;
	info!(
		"Map {} pages at {:#x} (page size {} KByte)",
		counter,
		page_address,
		BasePageSize::SIZE / 1024
	);
	paging::map::<BasePageSize>(
		page_address,
		page_address,
		counter,
		PageTableEntryFlags::empty(),
	);

	// map also the rest of the module
	let address = align_up!(start_address, LargePageSize::SIZE);
	let counter = (align_up!(end_address, LargePageSize::SIZE) - address) / LargePageSize::SIZE;
	if counter > 0 {
		info!(
			"Map {} pages at {:#x} (page size {} KByte)",
			counter,
			address,
			LargePageSize::SIZE / 1024
		);
		paging::map::<LargePageSize>(address, address, counter, PageTableEntryFlags::WRITABLE);
	}

	slice::from_raw_parts(elf_start as *const u8, elf_len)
}

#[cfg(target_os = "none")]
pub unsafe fn boot_kernel(kernel_info: LoadedKernel) -> ! {
	let LoadedKernel {
		load_info,
		entry_point,
	} = kernel_info;

	let multiboot = Multiboot::from_ptr(mb_info as u64, &mut MEM).unwrap();

	let command_line = multiboot.command_line().map(|command_line| {
		let address = command_line.as_ptr();

		// Identity-map the command line.
		let page_address = align_down!(address as usize, BasePageSize::SIZE);
		paging::map::<BasePageSize>(page_address, page_address, 1, PageTableEntryFlags::empty());

		command_line
	});

	// determine boot stack address
	let mut new_stack = align_up!(&kernel_end as *const u8 as usize, BasePageSize::SIZE);

	if new_stack + KERNEL_STACK_SIZE as usize > mb_info as usize {
		new_stack = align_up!(
			mb_info + mem::size_of::<Multiboot<'_, '_>>(),
			BasePageSize::SIZE
		);
	}

	if let Some(command_line) = command_line {
		let cmdline = command_line.as_ptr() as usize;
		let cmdsize = command_line.len();
		if new_stack + KERNEL_STACK_SIZE as usize > cmdline {
			new_stack = align_up!((cmdline + cmdsize), BasePageSize::SIZE);
		}
	}

	let current_stack_address = new_stack.try_into().unwrap();
	info!("Use stack address {:#x}", current_stack_address);

	// map stack in the address space
	paging::map::<BasePageSize>(
		new_stack,
		new_stack,
		KERNEL_STACK_SIZE as usize / BasePageSize::SIZE,
		PageTableEntryFlags::WRITABLE,
	);

	// clear stack
	write_bytes(
		new_stack as *mut u8,
		0,
		KERNEL_STACK_SIZE.try_into().unwrap(),
	);

	static mut BOOT_INFO: Option<RawBootInfo> = None;

	BOOT_INFO = {
		let boot_info = BootInfo {
			hardware_info: HardwareInfo {
				phys_addr_range: 0..0,
				serial_port_base: SerialPortBase::new(SERIAL_IO_PORT),
			},
			load_info,
			platform_info: PlatformInfo::Multiboot {
				command_line,
				multiboot_info_addr: (mb_info as u64).try_into().unwrap(),
			},
		};
		let raw_boot_info = RawBootInfo::from(boot_info);
		raw_boot_info.store_current_stack_address(current_stack_address);
		Some(raw_boot_info)
	};

	info!("BootInfo located at {:#x}", &BOOT_INFO as *const _ as u64);

	// Jump to the kernel entry point and provide the Multiboot information to it.
	info!(
		"Jumping to HermitCore Application Entry Point at {:#x}",
		entry_point
	);
	let func: Entry = core::mem::transmute(entry_point);
	func(BOOT_INFO.as_ref().unwrap(), 0);

	// we never reach this point
}

unsafe fn map_memory(address: usize, memory_size: usize) -> usize {
	let address = align_up!(address, LargePageSize::SIZE);
	let page_count = align_up!(memory_size, LargePageSize::SIZE) / LargePageSize::SIZE;

	paging::map::<LargePageSize>(address, address, page_count, PageTableEntryFlags::WRITABLE);

	address
}

pub unsafe fn get_memory(memory_size: u64) -> u64 {
	let address = physicalmem::allocate(align_up!(memory_size as usize, LargePageSize::SIZE));
	map_memory(address, memory_size as usize) as u64
}
