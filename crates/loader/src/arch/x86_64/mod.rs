pub mod bootinfo;
pub mod paging;
pub mod physicalmem;
pub mod serial;

pub use self::bootinfo::*;
use crate::arch::x86_64::paging::{BasePageSize, LargePageSize, PageSize, PageTableEntryFlags};
use crate::arch::x86_64::serial::SerialPort;
use core::ptr::{copy, write_bytes};
use core::{cmp, mem, slice};
use goblin::elf;
use multiboot::information::{MemoryManagement, Multiboot, PAddr};

extern "C" {
	static mb_info: usize;
	static kernel_end: u8;
}

// CONSTANTS
pub const ELF_ARCH: u16 = elf::header::EM_X86_64;

const KERNEL_STACK_SIZE: u64 = 32_768;
const SERIAL_PORT_ADDRESS: u16 = 0x3F8;
const SERIAL_PORT_BAUDRATE: u32 = 115200;

// VARIABLES
static COM1: SerialPort = SerialPort::new(SERIAL_PORT_ADDRESS);
pub static mut BOOT_INFO: BootInfo = BootInfo::new();

struct Mem;
static mut MEM: Mem = Mem;

impl MemoryManagement for Mem {
	unsafe fn paddr_to_slice<'a>(&self, p: PAddr, sz: usize) -> Option<&'static [u8]> {
		let ptr = mem::transmute(p);
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
	COM1.init(SERIAL_PORT_BAUDRATE);
}

pub fn output_message_byte(byte: u8) {
	COM1.write_byte(byte);
}

pub unsafe fn find_kernel() -> &'static [u8] {
	// Identity-map the Multiboot information.
	assert!(mb_info > 0, "Could not find Multiboot information");
	loaderlog!("Found Multiboot information at {:#x}", mb_info);
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

	loaderlog!("Found module: [{:#x} - {:#x}]", start_address, end_address);
	let elf_start = start_address;
	let elf_len = end_address - start_address;
	loaderlog!("Module length: {:#x}", elf_len);

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
	loaderlog!("Found an ELF module at {:#x}", start_address);
	let page_address = align_down!(start_address, BasePageSize::SIZE);
	let counter =
		(align_up!(start_address, LargePageSize::SIZE) - page_address) / BasePageSize::SIZE;
	loaderlog!(
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
		loaderlog!(
			"Map {} pages at {:#x} (page size {} KByte)",
			counter,
			address,
			LargePageSize::SIZE / 1024
		);
		paging::map::<LargePageSize>(address, address, counter, PageTableEntryFlags::WRITABLE);
	}

	slice::from_raw_parts(elf_start as *const u8, elf_len)
}

pub unsafe fn boot_kernel(
	elf_address: Option<u64>,
	virtual_address: u64,
	mem_size: u64,
	entry_point: u64,
) -> ! {
	let new_addr = match elf_address {
		Some(addr) => {
			if virtual_address != addr {
				loaderlog!("Copy kernel from {:#x} to {:#x}", virtual_address, addr);

				// copy app to the new start address
				copy(
					virtual_address as *const u8,
					addr as *mut u8,
					mem_size.try_into().unwrap(),
				);
			}

			addr
		}
		None => virtual_address,
	};

	// Supply the parameters to the HermitCore application.
	BOOT_INFO.base = new_addr;
	BOOT_INFO.image_size = mem_size;
	BOOT_INFO.mb_info = mb_info as u64;

	let multiboot = Multiboot::from_ptr(mb_info as u64, &mut MEM).unwrap();
	if let Some(cmdline) = multiboot.command_line() {
		let address = cmdline.as_ptr();

		// Identity-map the command line.
		let page_address = align_down!(address as usize, BasePageSize::SIZE);
		paging::map::<BasePageSize>(page_address, page_address, 1, PageTableEntryFlags::empty());

		//let cmdline = multiboot.command_line().unwrap();
		BOOT_INFO.cmdline = address as u64;
		BOOT_INFO.cmdsize = cmdline.len() as u64;
	}

	// determine boot stack address
	let mut new_stack = align_up!(&kernel_end as *const u8 as usize, BasePageSize::SIZE);

	if new_stack + KERNEL_STACK_SIZE as usize > mb_info as usize {
		new_stack = align_up!(
			mb_info + mem::size_of::<Multiboot<'_, '_>>(),
			BasePageSize::SIZE
		);
	}

	if new_stack + KERNEL_STACK_SIZE as usize > BOOT_INFO.cmdline as usize {
		new_stack = align_up!(
			(BOOT_INFO.cmdline + BOOT_INFO.cmdsize) as usize,
			BasePageSize::SIZE
		);
	}

	BOOT_INFO.current_stack_address = new_stack.try_into().unwrap();

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

	loaderlog!("BootInfo located at {:#x}", &BOOT_INFO as *const _ as u64);
	//loaderlog!("BootInfo {:?}", BOOT_INFO);
	loaderlog!("Use stack address {:#x}", BOOT_INFO.current_stack_address);

	// Jump to the kernel entry point and provide the Multiboot information to it.
	loaderlog!(
		"Jumping to HermitCore Application Entry Point at {:#x}",
		entry_point
	);
	let func: extern "C" fn(boot_info: &'static mut BootInfo) -> ! =
		core::mem::transmute(entry_point);

	func(&mut BOOT_INFO);

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
