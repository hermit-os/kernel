mod paging;
mod physicalmem;

use core::arch::asm;
#[cfg(all(target_os = "none", not(feature = "fc")))]
use core::mem;
#[cfg(target_os = "none")]
use core::ptr::write_bytes;
#[cfg(target_os = "none")]
use core::slice;

use hermit_entry::boot_info::{BootInfo, HardwareInfo, PlatformInfo, RawBootInfo, SerialPortBase};
use hermit_entry::elf::LoadedKernel;
#[cfg(all(target_os = "none", feature = "fc"))]
use hermit_entry::fc::{
	BOOT_FLAG_OFFSET, CMD_LINE_PTR_OFFSET, CMD_LINE_SIZE_OFFSET, E820_ENTRIES_OFFSET,
	E820_TABLE_OFFSET, HDR_MAGIC_OFFSET, LINUX_KERNEL_BOOT_FLAG_MAGIC, LINUX_KERNEL_HRD_MAGIC,
	LINUX_SETUP_HEADER_OFFSET, RAMDISK_IMAGE_OFFSET, RAMDISK_SIZE_OFFSET,
};
use hermit_entry::Entry;
use log::info;
#[cfg(all(target_os = "none", not(feature = "fc")))]
use multiboot::information::MemoryManagement;
#[cfg(all(target_os = "none", not(feature = "fc")))]
use multiboot::information::{Multiboot, PAddr};
use uart_16550::SerialPort;
use x86_64::structures::paging::{PageSize, PageTableFlags, Size2MiB, Size4KiB};

use self::physicalmem::PhysAlloc;

#[cfg(target_os = "none")]
extern "C" {
	static kernel_end: u8;
	#[cfg(feature = "fc")]
	static kernel_start: u8;
	#[cfg(not(feature = "fc"))]
	static mb_info: usize;
	#[cfg(feature = "fc")]
	static boot_params: usize;
}

// CONSTANTS
const KERNEL_STACK_SIZE: u64 = 32_768;
const SERIAL_IO_PORT: u16 = 0x3F8;

// VARIABLES
static mut COM1: SerialPort = unsafe { SerialPort::new(SERIAL_IO_PORT) };

#[cfg(all(target_os = "none", not(feature = "fc")))]
struct Mem;
#[cfg(all(target_os = "none", not(feature = "fc")))]
static mut MEM: Mem = Mem;

#[cfg(all(target_os = "none", not(feature = "fc")))]
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

#[cfg(all(target_os = "none", feature = "fc"))]
pub unsafe fn find_kernel() -> &'static [u8] {
	use core::cmp;

	paging::clean_up();

	// Identity-map the Multiboot information.
	assert!(boot_params > 0, "Could not find boot_params");
	info!("Found boot_params at 0x{:x}", boot_params);
	let page_address = align_down!(boot_params, Size4KiB::SIZE as usize);
	paging::map::<Size4KiB>(page_address, page_address, 1, PageTableFlags::empty());

	let linux_kernel_boot_flag_magic =
		*((&(boot_params as usize) + LINUX_SETUP_HEADER_OFFSET + BOOT_FLAG_OFFSET) as *const u16);
	let linux_kernel_header_magic =
		*((&(boot_params as usize) + LINUX_SETUP_HEADER_OFFSET + HDR_MAGIC_OFFSET) as *const u32);
	if linux_kernel_boot_flag_magic == LINUX_KERNEL_BOOT_FLAG_MAGIC
		&& linux_kernel_header_magic == LINUX_KERNEL_HRD_MAGIC
	{
		info!("Found Linux kernel boot flag and header magic! Probably booting in firecracker.");
	} else {
		info!("Kernel boot flag and hdr magic have values 0x{:x} and 0x{:x} which does not align with the normal linux kernel values", 
 			linux_kernel_boot_flag_magic,
 			linux_kernel_header_magic
 		);
	}

	// Load the boot_param memory-map information
	let linux_e820_entries = *((&(boot_params as usize) + E820_ENTRIES_OFFSET) as *const u8);
	info!("Number of e820-entries: {}", linux_e820_entries);

	let e820_entries_address = &(boot_params as usize) + E820_TABLE_OFFSET;
	info!("e820-entry-table at 0x{:x}", e820_entries_address);
	let page_address = align_down!(e820_entries_address, Size4KiB::SIZE as usize);

	if !(boot_params >= page_address && boot_params < page_address + Size4KiB::SIZE as usize) {
		paging::map::<Size4KiB>(page_address, page_address, 1, PageTableFlags::empty());
	}

	// Load the RustyHermit-ELF from the initrd supplied by Firecracker
	let ramdisk_address = *((&(boot_params as usize)
		+ LINUX_SETUP_HEADER_OFFSET
		+ RAMDISK_IMAGE_OFFSET) as *const u32);
	let ramdisk_size = *((&(boot_params as usize) + LINUX_SETUP_HEADER_OFFSET + RAMDISK_SIZE_OFFSET)
		as *const u32);

	info!(
		"Initrd: Address 0x{:x}, Size 0x{:x}",
		ramdisk_address, ramdisk_size
	);

	let elf_start = ramdisk_address as usize;
	let elf_len = ramdisk_size as usize;

	let free_memory_address = align_up!(&kernel_end as *const u8 as usize, Size2MiB::SIZE as usize);
	// TODO: Workaround for https://github.com/hermitcore/rusty-loader/issues/96
	let free_memory_address = cmp::max(free_memory_address, 0x800000);
	info!("Intialize PhysAlloc with {:#x}", free_memory_address);
	// Memory after the highest end address is unused and available for the physical memory manager.
	PhysAlloc::init(free_memory_address);

	assert!(ramdisk_address > 0);
	info!("Found an ELF module at {:#x}", elf_start);
	let page_address = align_down!(elf_start, Size4KiB::SIZE as usize);
	let counter =
		(align_up!(elf_start, Size2MiB::SIZE as usize) - page_address) / Size4KiB::SIZE as usize;
	paging::map::<Size4KiB>(page_address, page_address, counter, PageTableFlags::empty());

	// map also the rest of the module
	let address = align_up!(elf_start, Size2MiB::SIZE as usize);
	let counter = (align_up!(elf_start + elf_len, Size2MiB::SIZE as usize) - address)
		/ Size2MiB::SIZE as usize;
	if counter > 0 {
		paging::map::<Size2MiB>(address, address, counter, PageTableFlags::empty());
	}

	slice::from_raw_parts(elf_start as *const u8, elf_len)
}

#[cfg(all(target_os = "none", not(feature = "fc")))]
pub unsafe fn find_kernel() -> &'static [u8] {
	use core::cmp;

	paging::clean_up();
	// Identity-map the Multiboot information.
	assert!(mb_info > 0, "Could not find Multiboot information");
	info!("Found Multiboot information at {:#x}", mb_info);
	let page_address = align_down!(mb_info, Size4KiB::SIZE as usize);
	paging::map::<Size4KiB>(page_address, page_address, 1, PageTableFlags::empty());

	// Load the Multiboot information and identity-map the modules information.
	let multiboot = Multiboot::from_ptr(mb_info as u64, &mut MEM).unwrap();
	let modules_address = multiboot
		.modules()
		.expect("Could not find a memory map in the Multiboot information")
		.next()
		.expect("Could not find first map address")
		.start as usize;
	let page_address = align_down!(modules_address, Size4KiB::SIZE as usize);
	paging::map::<Size4KiB>(page_address, page_address, 1, PageTableFlags::empty());

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

	let free_memory_address = align_up!(end_address, Size2MiB::SIZE as usize);
	// TODO: Workaround for https://github.com/hermitcore/rusty-loader/issues/96
	let free_memory_address = cmp::max(free_memory_address, 0x800000);
	// Memory after the highest end address is unused and available for the physical memory manager.
	PhysAlloc::init(free_memory_address);

	// Identity-map the ELF header of the first module.
	assert!(
		found_module,
		"Could not find a single module in the Multiboot information"
	);
	assert!(start_address > 0);
	info!("Found an ELF module at {:#x}", start_address);
	let page_address =
		align_down!(start_address, Size4KiB::SIZE as usize) + Size4KiB::SIZE as usize;
	let counter = (align_up!(start_address, Size2MiB::SIZE as usize) - page_address)
		/ Size4KiB::SIZE as usize;
	paging::map::<Size4KiB>(page_address, page_address, counter, PageTableFlags::empty());

	// map also the rest of the module
	let address = align_up!(start_address, Size2MiB::SIZE as usize);
	let counter =
		(align_up!(end_address, Size2MiB::SIZE as usize) - address) / Size2MiB::SIZE as usize;
	if counter > 0 {
		paging::map::<Size2MiB>(address, address, counter, PageTableFlags::empty());
	}

	slice::from_raw_parts(elf_start as *const u8, elf_len)
}

#[cfg(all(target_os = "none", feature = "fc"))]
pub unsafe fn boot_kernel(kernel_info: LoadedKernel) -> ! {
	let LoadedKernel {
		load_info,
		entry_point,
	} = kernel_info;

	// determine boot stack address
	let new_stack = align_up!(
		&kernel_end as *const u8 as usize + 0x1000,
		Size4KiB::SIZE as usize
	);

	let cmdline_ptr = *((&(boot_params as usize) + LINUX_SETUP_HEADER_OFFSET + CMD_LINE_PTR_OFFSET)
		as *const u32);
	let cmdline_size = *((&(boot_params as usize)
		+ LINUX_SETUP_HEADER_OFFSET
		+ CMD_LINE_SIZE_OFFSET) as *const u32);

	let command_line = if cmdline_size > 0 {
		// Identity-map the command line.
		let page_address = align_down!(cmdline_ptr as usize, Size4KiB::SIZE as usize);
		paging::map::<Size4KiB>(page_address, page_address, 1, PageTableFlags::empty());

		info!("Found command line at {:#x}", cmdline_ptr);
		let slice = unsafe {
			core::slice::from_raw_parts(cmdline_ptr as *const u8, cmdline_size.try_into().unwrap())
		};

		Some(core::str::from_utf8(slice).unwrap())
	} else {
		None
	};

	let current_stack_address = new_stack as u64;
	info!(
		"Use kernel stack:  [{:#x} - {:#x}]",
		current_stack_address,
		current_stack_address + KERNEL_STACK_SIZE
	);

	// map stack in the address space
	paging::map::<Size4KiB>(
		new_stack,
		new_stack,
		KERNEL_STACK_SIZE as usize / Size4KiB::SIZE as usize,
		PageTableFlags::WRITABLE,
	);

	// clear stack
	write_bytes(
		new_stack as *mut u8,
		0,
		KERNEL_STACK_SIZE.try_into().unwrap(),
	);

	// Load the boot_param memory-map information
	let linux_e820_entries = *((&(boot_params as usize) + E820_ENTRIES_OFFSET) as *const u8);
	info!("Number of e820-entries: {}", linux_e820_entries);

	let mut found_entry = false;
	let mut start_address: usize = 0;
	let mut end_address: usize = 0;

	let e820_entries_address = &(boot_params as usize) + E820_TABLE_OFFSET;

	for index in 0..linux_e820_entries {
		found_entry = true;

		//20: Size of one e820-Entry
		let entry_address = e820_entries_address + (index as usize) * 20;
		let entry_start = *(entry_address as *const u64);
		let entry_size = *((entry_address + 8) as *const u64);
		let entry_type = *((entry_address + 16) as *const u32);

		info!(
			"e820-Entry with index {}: Address 0x{:x}, Size 0x{:x}, Type 0x{:x}",
			index, entry_start, entry_size, entry_type
		);

		let entry_end = entry_start + entry_size;

		if start_address == 0 {
			start_address = entry_start as usize;
		}

		if entry_end as usize > end_address {
			end_address = entry_end as usize;
		}
	}

	// Identity-map the start of RAM
	assert!(found_entry, "Could not find any free RAM areas!");

	info!(
		"Found available RAM: [0x{:x} - 0x{:x}]",
		start_address, end_address
	);

	static mut BOOT_INFO: Option<RawBootInfo> = None;

	BOOT_INFO = {
		let boot_info = BootInfo {
			hardware_info: HardwareInfo {
				phys_addr_range: start_address as u64..end_address as u64,
				serial_port_base: SerialPortBase::new(SERIAL_IO_PORT),
				device_tree: None,
			},
			load_info,
			platform_info: PlatformInfo::LinuxBootParams {
				command_line,
				boot_params_addr: (boot_params as u64).try_into().unwrap(),
			},
		};
		Some(RawBootInfo::from(boot_info))
	};

	info!("BootInfo located at {:#x}", &BOOT_INFO as *const _ as u64);

	// Jump to the kernel entry point and provide the Multiboot information to it.
	info!(
		"Jumping to HermitCore Application Entry Point at {:#x}",
		entry_point
	);

	#[allow(dead_code)]
	const ENTRY_TYPE_CHECK: Entry = {
		unsafe extern "C" fn entry_signature(
			_raw_boot_info: &'static RawBootInfo,
			_cpu_id: u32,
		) -> ! {
			unimplemented!()
		}
		entry_signature
	};

	asm!(
		"mov rsp, {stack_address}",
		"jmp {entry}",
		stack_address = in(reg) current_stack_address,
		entry = in(reg) entry_point,
		in("rdi") BOOT_INFO.as_ref().unwrap(),
		in("rsi") 0,
		options(noreturn)
	)
}

#[cfg(all(target_os = "none", not(feature = "fc")))]
pub unsafe fn boot_kernel(kernel_info: LoadedKernel) -> ! {
	let LoadedKernel {
		load_info,
		entry_point,
	} = kernel_info;

	let multiboot = Multiboot::from_ptr(mb_info as u64, &mut MEM).unwrap();

	// determine boot stack address
	let mut new_stack = align_up!(&kernel_end as *const u8 as usize, Size4KiB::SIZE as usize);

	if new_stack + KERNEL_STACK_SIZE as usize > mb_info {
		new_stack = align_up!(
			mb_info + mem::size_of::<Multiboot<'_, '_>>(),
			Size4KiB::SIZE as usize
		);
	}

	let command_line = multiboot.command_line();
	if let Some(command_line) = command_line {
		let cmdline = command_line.as_ptr() as usize;
		let cmdsize = command_line.len();
		if new_stack + KERNEL_STACK_SIZE as usize > cmdline {
			new_stack = align_up!((cmdline + cmdsize), Size4KiB::SIZE as usize);
		}
	}

	let current_stack_address = new_stack as u64;
	info!("Use stack address {:#x}", current_stack_address);

	// map stack in the address space
	paging::map::<Size4KiB>(
		new_stack,
		new_stack,
		KERNEL_STACK_SIZE as usize / Size4KiB::SIZE as usize,
		PageTableFlags::WRITABLE,
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
				device_tree: None,
			},
			load_info,
			platform_info: PlatformInfo::Multiboot {
				command_line,
				multiboot_info_addr: (mb_info as u64).try_into().unwrap(),
			},
		};
		Some(RawBootInfo::from(boot_info))
	};

	info!("BootInfo located at {:#x}", &BOOT_INFO as *const _ as u64);

	// Jump to the kernel entry point and provide the Multiboot information to it.
	info!(
		"Jumping to HermitCore Application Entry Point at {:#x}",
		entry_point
	);

	#[allow(dead_code)]
	const ENTRY_TYPE_CHECK: Entry = {
		unsafe extern "C" fn entry_signature(
			_raw_boot_info: &'static RawBootInfo,
			_cpu_id: u32,
		) -> ! {
			unimplemented!()
		}
		entry_signature
	};

	asm!(
		"mov rsp, {stack_address}",
		"jmp {entry}",
		stack_address = in(reg) current_stack_address,
		entry = in(reg) entry_point,
		in("rdi") BOOT_INFO.as_ref().unwrap(),
		in("rsi") 0,
		options(noreturn)
	)
}

unsafe fn map_memory(address: usize, memory_size: usize) -> usize {
	let address = align_up!(address, Size2MiB::SIZE as usize);
	let page_count = align_up!(memory_size, Size2MiB::SIZE as usize) / Size2MiB::SIZE as usize;

	paging::map::<Size2MiB>(address, address, page_count, PageTableFlags::WRITABLE);

	address
}

pub unsafe fn get_memory(memory_size: u64) -> u64 {
	let address = PhysAlloc::allocate(align_up!(memory_size as usize, Size2MiB::SIZE as usize));
	map_memory(address, memory_size as usize) as u64
}
