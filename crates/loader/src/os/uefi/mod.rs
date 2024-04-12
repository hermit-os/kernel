mod allocator;
mod console;
mod fdt;

use alloc::vec::Vec;
use core::ffi::c_void;
use core::mem::MaybeUninit;
use core::slice;

use align_address::Align;
use hermit_entry::boot_info::{
	BootInfo, DeviceTreeAddress, HardwareInfo, PlatformInfo, SerialPortBase,
};
use hermit_entry::elf::{KernelObject, LoadedKernel};
use log::info;
use sptr::Strict;
use uefi::boot::{AllocateType, MemoryType, PAGE_SIZE};
use uefi::fs::{FileSystem, Path};
use uefi::prelude::*;
use uefi::table::cfg;

pub use self::console::CONSOLE;
use self::fdt::Fdt;
use crate::{arch, BootInfoExt};

// Entry Point of the Uefi Loader
#[entry]
fn main() -> Status {
	uefi::helpers::init().unwrap();
	crate::log::init();

	let kernel_image = read_app();
	let kernel = KernelObject::parse(&kernel_image).unwrap();

	let kernel_memory = alloc_page_slice(kernel.mem_size()).unwrap();
	let kernel_memory = &mut kernel_memory[..kernel.mem_size()];

	let kernel_info = kernel.load_kernel(kernel_memory, kernel_memory.as_ptr() as u64);

	let rsdp = rsdp();

	drop(kernel_image);

	let fdt = Fdt::new()
		.unwrap()
		.rsdp(u64::try_from(rsdp.expose_addr()).unwrap())
		.unwrap();

	allocator::exit_boot_services();
	let mut memory_map = unsafe { boot::exit_boot_services(MemoryType::LOADER_DATA) };

	let fdt = fdt.memory_map(&mut memory_map).unwrap().finish().unwrap();

	unsafe { boot_kernel(kernel_info, fdt) }
}

fn read_app() -> Vec<u8> {
	let image_handle = boot::image_handle();
	let fs = boot::get_image_file_system(image_handle).expect("should open file system");

	let path = Path::new(cstr16!(r"\efi\boot\hermit-app"));

	let data = FileSystem::new(fs)
		.read(path)
		.expect("should read file content");

	let len = data.len();
	info!("Read Hermit application from \"{path}\" (size = {len} B)");

	data
}

pub unsafe fn boot_kernel(kernel_info: LoadedKernel, fdt: Vec<u8>) -> ! {
	let LoadedKernel {
		load_info,
		entry_point,
	} = kernel_info;

	let device_tree = DeviceTreeAddress::new(u64::try_from(fdt.leak().as_ptr().addr()).unwrap());

	let boot_info = BootInfo {
		hardware_info: HardwareInfo {
			phys_addr_range: 0..0,
			serial_port_base: SerialPortBase::new(arch::SERIAL_IO_PORT),
			device_tree,
		},
		load_info,
		platform_info: PlatformInfo::Fdt,
	};

	let stack = usize::try_from(boot_info.load_info.kernel_image_addr_range.end)
		.unwrap()
		.align_down(PAGE_SIZE);
	let entry = sptr::from_exposed_addr(entry_point.try_into().unwrap());
	let stack = sptr::from_exposed_addr_mut(stack);
	let raw_boot_info = boot_info.write();

	unsafe { arch::enter_kernel(stack, entry, raw_boot_info) }
}

fn alloc_page_slice(size: usize) -> uefi::Result<&'static mut [MaybeUninit<u8>]> {
	let size = size.align_up(PAGE_SIZE);
	let ptr = boot::allocate_pages(
		AllocateType::AnyPages,
		MemoryType::LOADER_DATA,
		size / PAGE_SIZE,
	)?;
	Ok(unsafe { slice::from_raw_parts_mut(ptr.cast().as_ptr(), size) })
}

/// Returns the RSDP.
///
/// This must be called before exiting boot services.
/// See [5.2.5.2. Finding the RSDP on UEFI Enabled Systems — ACPI Specification 6.5 documentation](https://uefi.org/specs/ACPI/6.5/05_ACPI_Software_Programming_Model.html#finding-the-rsdp-on-uefi-enabled-systems) for details.
fn rsdp() -> *const c_void {
	system::with_config_table(|config_table| {
		let (rsdp, version) = if let Some(entry) = config_table
			.iter()
			.find(|entry| entry.guid == cfg::ACPI2_GUID)
		{
			(entry.address, 2)
		} else {
			let entry = config_table
				.iter()
				.find(|entry| entry.guid == cfg::ACPI_GUID)
				.unwrap();
			(entry.address, 1)
		};
		info!("Found ACPI {version} RSDP at {rsdp:p}");
		rsdp
	})
}
