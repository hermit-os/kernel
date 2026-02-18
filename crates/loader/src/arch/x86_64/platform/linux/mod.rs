use alloc::borrow::ToOwned;
use core::ffi::CStr;
use core::ptr::write_bytes;
use core::sync::atomic::{AtomicPtr, Ordering};
use core::{ptr, slice};

use align_address::Align;
use hermit_entry::boot_info::{
	BootInfo, DeviceTreeAddress, HardwareInfo, PlatformInfo, SerialPortBase,
};
use hermit_entry::elf::LoadedKernel;
use linux_boot_params::{BootE820Entry, BootParams};
use log::{error, info};
use x86_64::structures::paging::{PageSize, PageTableFlags, Size2MiB, Size4KiB};

use crate::BootInfoExt;
use crate::arch::x86_64::physicalmem::PhysAlloc;
use crate::arch::x86_64::{KERNEL_STACK_SIZE, SERIAL_IO_PORT, paging};
use crate::fdt::Fdt;

unsafe extern "C" {
	static mut loader_end: u8;
}

mod entry {
	core::arch::global_asm!(
		include_str!("entry.s"),
		rust_start = sym super::rust_start,
		stack = sym crate::arch::x86_64::stack::STACK,
		stack_top_offset = const crate::arch::x86_64::stack::Stack::top_offset(),
		level_4_table = sym crate::arch::x86_64::page_tables::LEVEL_4_TABLE,
		gdt_ptr = sym crate::arch::x86_64::gdt::GDT_PTR,
		kernel_code_selector = const crate::arch::x86_64::gdt::Gdt::kernel_code_selector().0,
		kernel_data_selector = const crate::arch::x86_64::gdt::Gdt::kernel_data_selector().0,
	);
}

static BOOT_PARAMS: AtomicPtr<BootParams> = AtomicPtr::new(ptr::null_mut());

unsafe extern "C" fn rust_start(boot_params: *mut BootParams) -> ! {
	crate::log::init();
	BOOT_PARAMS.store(boot_params, Ordering::Relaxed);
	unsafe {
		crate::os::loader_main();
	}
}

pub fn find_kernel() -> &'static [u8] {
	paging::clean_up();

	unsafe {
		BootParams::map();
	}
	let boot_params_ref = unsafe { BootParams::get() };

	assert!(boot_params_ref.supported());

	let free_addr = ptr::addr_of!(loader_end)
		.addr()
		.align_up(Size2MiB::SIZE as usize);
	// Memory after the highest end address is unused and available for the physical memory manager.
	info!("Intializing PhysAlloc with {free_addr:#x}");
	PhysAlloc::init(free_addr);

	boot_params_ref.map_ramdisk().unwrap()
}

pub unsafe fn boot_kernel(kernel_info: LoadedKernel) -> ! {
	let LoadedKernel {
		load_info,
		entry_point,
	} = kernel_info;

	let boot_params_ref = unsafe { BootParams::get() };

	// determine boot stack address
	let stack = (ptr::addr_of!(loader_end).addr() + Size4KiB::SIZE as usize)
		.align_up(Size4KiB::SIZE as usize);
	paging::map::<Size4KiB>(
		stack,
		stack,
		KERNEL_STACK_SIZE as usize / Size4KiB::SIZE as usize,
		PageTableFlags::WRITABLE,
	);
	let stack = ptr::addr_of_mut!(loader_end).with_addr(stack);
	// clear stack
	unsafe {
		write_bytes(stack, 0, KERNEL_STACK_SIZE.try_into().unwrap());
	}

	let mut fdt = Fdt::new("linux").unwrap();

	let e820_entries = boot_params_ref.e820_entries();
	assert!(!e820_entries.is_empty());

	for entry in e820_entries.iter().copied() {
		let BootE820Entry { addr, size, typ } = entry;
		info!("E820 memory region: addr = {addr:>#11x}, size = {size:>#11x}, type = {typ:?}");
		let memory = addr..addr + size;
		fdt = fdt.memory(memory).unwrap();
	}

	let start = e820_entries
		.iter()
		.copied()
		.map(|entry| entry.addr)
		.find(|addr| *addr != 0)
		.unwrap();

	let end = e820_entries
		.iter()
		.copied()
		.map(|entry| entry.addr + entry.size)
		.max()
		.unwrap();

	let phys_addr_range = start..end;

	let command_line = boot_params_ref.map_cmdline().to_str().unwrap();
	fdt = fdt.bootargs(command_line.to_owned()).unwrap();

	let fdt = fdt.finish().unwrap();

	let device_tree =
		DeviceTreeAddress::new(u64::try_from(fdt.leak().as_ptr().expose_provenance()).unwrap());

	let boot_info = BootInfo {
		hardware_info: HardwareInfo {
			phys_addr_range,
			serial_port_base: SerialPortBase::new(SERIAL_IO_PORT),
			device_tree,
		},
		load_info,
		platform_info: PlatformInfo::LinuxBootParams {
			command_line: Some(command_line),
			boot_params_addr: u64::try_from(
				BOOT_PARAMS.load(Ordering::Relaxed).expose_provenance(),
			)
			.unwrap()
			.try_into()
			.unwrap(),
		},
	};

	let entry = ptr::with_exposed_provenance(entry_point.try_into().unwrap());
	let raw_boot_info = boot_info.write();

	unsafe { crate::arch::x86_64::enter_kernel(stack, entry, raw_boot_info) }
}

trait BootParamsExt {
	unsafe fn map();
	unsafe fn get() -> &'static Self;
	fn supported(&self) -> bool;
	fn map_ramdisk(&self) -> Option<&[u8]>;
	fn map_cmdline(&self) -> &CStr;
	fn e820_entries(&self) -> &[BootE820Entry];
}

impl BootParamsExt for BootParams {
	unsafe fn map() {
		let ptr = BOOT_PARAMS.load(Ordering::Relaxed);

		info!("Linux boot parameters: {ptr:p}");
		let addr = ptr.expose_provenance();
		assert!(addr.is_aligned_to(Size4KiB::SIZE as usize));
		assert_ne!(addr, 0);

		// Identity-map the boot parameters.
		paging::map::<Size4KiB>(addr, addr, 1, PageTableFlags::empty());
	}

	unsafe fn get() -> &'static Self {
		let ptr = BOOT_PARAMS.load(Ordering::Relaxed);
		unsafe { &*ptr }
	}

	fn supported(&self) -> bool {
		let boot_flag = self.hdr.boot_flag;
		let boot_flag_expected = 0xaa55;
		if boot_flag != boot_flag_expected {
			error!("The boot flag is invalid.");
			error!("Expected {boot_flag_expected:#x}. Got {boot_flag:#x}.");
			return false;
		}

		let header = self.hdr.header;
		let header_expected = u32::from_le_bytes(*b"HdrS");
		if header != header_expected {
			error!("This old Linux boot protocol version is not supported.");
			return false;
		}

		true
	}

	fn map_ramdisk(&self) -> Option<&[u8]> {
		let ramdisk_image = self.hdr.ramdisk_image as usize;
		let ramdisk_size = self.hdr.ramdisk_size as usize;
		info!("ramdisk_image = {ramdisk_image:#x}");
		info!("ramdisk_size = {ramdisk_size:#x}");
		if ramdisk_image == 0 {
			return None;
		}
		if ramdisk_size == 0 {
			return None;
		}
		assert!(ramdisk_image.is_aligned_to(Size4KiB::SIZE as usize));

		// Map the start of the image in 4KiB steps.
		let count = (ramdisk_image.align_up(Size2MiB::SIZE as usize) - ramdisk_image)
			/ Size4KiB::SIZE as usize;
		if count > 0 {
			paging::map::<Size4KiB>(ramdisk_image, ramdisk_image, count, PageTableFlags::empty());
		}

		// Map the rest of the image in 2MiB steps.
		let addr = ramdisk_image.align_up(Size2MiB::SIZE as usize);
		let count = ((ramdisk_image + ramdisk_size).align_up(Size2MiB::SIZE as usize) - addr)
			/ Size2MiB::SIZE as usize;
		if count > 0 {
			paging::map::<Size2MiB>(addr, addr, count, PageTableFlags::empty());
		}

		let ramdisk_ptr = ptr::with_exposed_provenance(ramdisk_image);
		let ramdisk = unsafe { slice::from_raw_parts(ramdisk_ptr, ramdisk_size) };
		Some(ramdisk)
	}

	fn map_cmdline(&self) -> &CStr {
		let cmd_line_ptr = self.hdr.cmd_line_ptr as usize;
		let cmdline_size = self.hdr.cmdline_size as usize;
		info!("cmd_line_ptr = {cmd_line_ptr:#x}");
		info!("cmdline_size = {cmdline_size:#x}");
		assert_ne!(cmd_line_ptr, 0, "boot protocol is older than 2.02");
		assert!(cmd_line_ptr.is_aligned_to(Size4KiB::SIZE as usize));

		paging::map::<Size4KiB>(cmd_line_ptr, cmd_line_ptr, 1, PageTableFlags::empty());

		let ptr = ptr::with_exposed_provenance(cmd_line_ptr);
		let bytes = unsafe { core::slice::from_raw_parts(ptr, cmdline_size) };
		CStr::from_bytes_until_nul(bytes).unwrap()
	}

	fn e820_entries(&self) -> &[BootE820Entry] {
		let e820_entries = self.e820_entries as usize;
		&self.e820_table[..e820_entries]
	}
}
