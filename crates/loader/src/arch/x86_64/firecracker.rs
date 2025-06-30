use core::ptr::write_bytes;
use core::{ptr, slice};

use align_address::Align;
use hermit_entry::boot_info::{
	BootInfo, DeviceTreeAddress, HardwareInfo, PlatformInfo, SerialPortBase,
};
use hermit_entry::elf::LoadedKernel;
use hermit_entry::fc::{
	BOOT_FLAG_OFFSET, CMD_LINE_PTR_OFFSET, CMD_LINE_SIZE_OFFSET, E820_ENTRIES_OFFSET,
	E820_TABLE_OFFSET, HDR_MAGIC_OFFSET, LINUX_KERNEL_BOOT_FLAG_MAGIC, LINUX_KERNEL_HRD_MAGIC,
	LINUX_SETUP_HEADER_OFFSET, RAMDISK_IMAGE_OFFSET, RAMDISK_SIZE_OFFSET,
};
use log::info;
use sptr::Strict;
use x86_64::structures::paging::{PageSize, PageTableFlags, Size2MiB, Size4KiB};

use super::physicalmem::PhysAlloc;
use super::{KERNEL_STACK_SIZE, SERIAL_IO_PORT, paging};
use crate::BootInfoExt;
use crate::fdt::Fdt;

unsafe extern "C" {
	static mut loader_end: u8;
	static boot_params: usize;
}

mod entry {
	core::arch::global_asm!(
		include_str!("entry_fc.s"),
		loader_main = sym crate::os::loader_main,
	);
}

pub fn find_kernel() -> &'static [u8] {
	use core::cmp;

	paging::clean_up();

	// Identity-map the Multiboot information.
	unsafe {
		assert!(boot_params > 0, "Could not find boot_params");
		info!("Found boot_params at {boot_params:#x}");
	}
	let page_address = unsafe { boot_params }.align_down(Size4KiB::SIZE as usize);
	paging::map::<Size4KiB>(page_address, page_address, 1, PageTableFlags::empty());

	let linux_kernel_boot_flag_magic: u16 = unsafe {
		*(sptr::from_exposed_addr(boot_params + LINUX_SETUP_HEADER_OFFSET + BOOT_FLAG_OFFSET))
	};
	let linux_kernel_header_magic = unsafe {
		sptr::from_exposed_addr::<u32>(boot_params + LINUX_SETUP_HEADER_OFFSET + HDR_MAGIC_OFFSET)
			.read_unaligned()
	};
	if linux_kernel_boot_flag_magic == LINUX_KERNEL_BOOT_FLAG_MAGIC
		&& linux_kernel_header_magic == LINUX_KERNEL_HRD_MAGIC
	{
		info!("Found Linux kernel boot flag and header magic! Probably booting in firecracker.");
	} else {
		info!(
			"Kernel boot flag and hdr magic have values {linux_kernel_boot_flag_magic:#x} and {linux_kernel_header_magic:#x} which does not align with the normal linux kernel values"
		);
	}

	// Load the boot_param memory-map information
	let linux_e820_entries: u8 =
		unsafe { *(sptr::from_exposed_addr(boot_params + E820_ENTRIES_OFFSET)) };
	info!("Number of e820-entries: {linux_e820_entries}");

	let e820_entries_address = unsafe { boot_params } + E820_TABLE_OFFSET;
	info!("e820-entry-table at {e820_entries_address:#x}");
	let page_address = e820_entries_address.align_down(Size4KiB::SIZE as usize);

	if !(unsafe { boot_params } >= page_address
		&& unsafe { boot_params } < page_address + Size4KiB::SIZE as usize)
	{
		paging::map::<Size4KiB>(page_address, page_address, 1, PageTableFlags::empty());
	}

	// Load the Hermit-ELF from the initrd supplied by Firecracker
	let ramdisk_address: u32 = unsafe {
		*(sptr::from_exposed_addr(boot_params + LINUX_SETUP_HEADER_OFFSET + RAMDISK_IMAGE_OFFSET))
	};
	let ramdisk_size: u32 = unsafe {
		*(sptr::from_exposed_addr(boot_params + LINUX_SETUP_HEADER_OFFSET + RAMDISK_SIZE_OFFSET))
	};

	info!("Initrd: Address {ramdisk_address:#x}, Size {ramdisk_size:#x}");

	let elf_start = ramdisk_address as usize;
	let elf_len = ramdisk_size as usize;

	let free_memory_address = ptr::addr_of!(loader_end)
		.addr()
		.align_up(Size2MiB::SIZE as usize);
	// TODO: Workaround for https://github.com/hermitcore/loader/issues/96
	let free_memory_address = cmp::max(free_memory_address, 0x800000);
	info!("Intialize PhysAlloc with {free_memory_address:#x}");
	// Memory after the highest end address is unused and available for the physical memory manager.
	PhysAlloc::init(free_memory_address);

	assert!(ramdisk_address > 0);
	info!("Found an ELF module at {elf_start:#x}");
	let page_address = elf_start.align_down(Size4KiB::SIZE as usize);
	let counter =
		(elf_start.align_up(Size2MiB::SIZE as usize) - page_address) / Size4KiB::SIZE as usize;
	paging::map::<Size4KiB>(page_address, page_address, counter, PageTableFlags::empty());

	// map also the rest of the module
	let address = elf_start.align_up(Size2MiB::SIZE as usize);
	let counter = ((elf_start + elf_len).align_up(Size2MiB::SIZE as usize) - address)
		/ Size2MiB::SIZE as usize;
	if counter > 0 {
		paging::map::<Size2MiB>(address, address, counter, PageTableFlags::empty());
	}

	unsafe { slice::from_raw_parts(sptr::from_exposed_addr(elf_start), elf_len) }
}

pub unsafe fn boot_kernel(kernel_info: LoadedKernel) -> ! {
	let LoadedKernel {
		load_info,
		entry_point,
	} = kernel_info;

	// determine boot stack address
	let new_stack = (ptr::addr_of!(loader_end).addr() + 0x1000).align_up(Size4KiB::SIZE as usize);

	let cmdline_ptr = unsafe {
		*(sptr::from_exposed_addr(boot_params + LINUX_SETUP_HEADER_OFFSET + CMD_LINE_PTR_OFFSET))
	};
	let cmdline_size: u32 = unsafe {
		*(sptr::from_exposed_addr(boot_params + LINUX_SETUP_HEADER_OFFSET + CMD_LINE_SIZE_OFFSET))
	};

	let command_line = if cmdline_size > 0 {
		// Identity-map the command line.
		let page_address = (cmdline_ptr as usize).align_down(Size4KiB::SIZE as usize);
		paging::map::<Size4KiB>(page_address, page_address, 1, PageTableFlags::empty());

		info!("Found command line at {cmdline_ptr:#x}");
		let slice = unsafe {
			core::slice::from_raw_parts(
				sptr::from_exposed_addr(cmdline_ptr),
				cmdline_size.try_into().unwrap(),
			)
		};

		let s = core::str::from_utf8(slice)
			.unwrap()
			.strip_suffix('\0')
			.unwrap();

		if s.is_empty() { None } else { Some(s) }
	} else {
		None
	};

	// map stack in the address space
	paging::map::<Size4KiB>(
		new_stack,
		new_stack,
		KERNEL_STACK_SIZE as usize / Size4KiB::SIZE as usize,
		PageTableFlags::WRITABLE,
	);

	let stack = ptr::addr_of_mut!(loader_end).with_addr(new_stack);

	// clear stack
	unsafe {
		write_bytes(stack, 0, KERNEL_STACK_SIZE.try_into().unwrap());
	}

	let mut fdt = Fdt::new("firecracker").unwrap();

	// Load the boot_param memory-map information
	let linux_e820_entries =
		unsafe { *(sptr::from_exposed_addr::<u8>(boot_params + E820_ENTRIES_OFFSET)) };
	info!("Number of e820-entries: {linux_e820_entries}");

	let mut found_entry = false;
	let mut start_address: usize = 0;
	let mut end_address: usize = 0;

	let e820_entries_address = unsafe { boot_params } + E820_TABLE_OFFSET;

	for index in 0..linux_e820_entries {
		found_entry = true;

		//20: Size of one e820-Entry
		let entry_address = e820_entries_address + (index as usize) * 20;
		let entry_start = unsafe { sptr::from_exposed_addr::<u64>(entry_address).read_unaligned() };
		let entry_size =
			unsafe { sptr::from_exposed_addr::<u64>(entry_address + 8).read_unaligned() };
		let entry_type: u32 = unsafe { sptr::from_exposed_addr::<u32>(entry_address + 16).read() };

		info!(
			"e820-Entry with index {index}: Address {entry_start:#x}, Size {entry_size:#x}, Type {entry_type:#x}"
		);

		let entry_end = entry_start + entry_size;

		fdt = fdt.memory(entry_start..entry_end).unwrap();

		if start_address == 0 {
			start_address = entry_start as usize;
		}

		if entry_end as usize > end_address {
			end_address = entry_end as usize;
		}
	}

	// Identity-map the start of RAM
	assert!(found_entry, "Could not find any free RAM areas!");

	info!("Found available RAM: [{start_address:#x} - {end_address:#x}]");

	if let Some(command_line) = command_line {
		fdt = fdt.bootargs(command_line).unwrap();
	}

	let fdt = fdt.finish().unwrap();

	let device_tree =
		DeviceTreeAddress::new(u64::try_from(fdt.leak().as_ptr().expose_addr()).unwrap());

	let boot_info = BootInfo {
		hardware_info: HardwareInfo {
			phys_addr_range: start_address as u64..end_address as u64,
			serial_port_base: SerialPortBase::new(SERIAL_IO_PORT),
			device_tree,
		},
		load_info,
		platform_info: PlatformInfo::LinuxBootParams {
			command_line,
			boot_params_addr: (unsafe { boot_params } as u64).try_into().unwrap(),
		},
	};

	let entry = sptr::from_exposed_addr(entry_point.try_into().unwrap());
	let raw_boot_info = boot_info.write();

	unsafe { super::enter_kernel(stack, entry, raw_boot_info) }
}
