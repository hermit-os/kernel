mod allocator;
mod console;

use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::mem::MaybeUninit;
use core::{ptr, slice};

use align_address::Align;
use anyhow::anyhow;
use hermit_entry::boot_info::{
	BootInfo, DeviceTreeAddress, HardwareInfo, PlatformInfo, SerialPortBase,
};
use hermit_entry::elf::{KernelObject, LoadedKernel};
use log::{error, info};
use uefi::boot::{AllocateType, MemoryType, PAGE_SIZE, open_protocol_exclusive};
use uefi::fs::{self, FileSystem, Path};
use uefi::prelude::*;
use uefi::proto::loaded_image::LoadedImage;
use uefi::table::cfg::ConfigTableEntry;
use uefi::{CString16, Guid, guid};

pub use self::console::CONSOLE;
use crate::fdt::Fdt;
use crate::{BootInfoExt, arch};

// Entry Point of the Uefi Loader
#[entry]
fn main() -> Status {
	uefi::helpers::init().unwrap();
	crate::log::init();

	let kernel_args = KernelArguments::new().unwrap();
	let mut esp = BootPartition::new().unwrap();

	let kernel_image = if let Some(path) = kernel_args
		.as_ref()
		.and_then(|arg| arg.initrd_path.as_ref())
	{
		esp.read_app_at(path.as_ref())
			.expect("Could not open kernel image provided in initrd")
	} else {
		esp.read_app()
	};
	let kernel = KernelObject::parse(&kernel_image).unwrap();

	let kernel_memory = alloc_page_slice(kernel.mem_size()).unwrap();
	let kernel_memory = &mut kernel_memory[..kernel.mem_size()];

	let kernel_info = kernel.load_kernel(kernel_memory, kernel_memory.as_ptr() as u64);

	let rsdp = rsdp();

	drop(kernel_image);

	let mut fdt = Fdt::new("uefi")
		.unwrap()
		.rsdp(u64::try_from(rsdp.expose_provenance()).unwrap())
		.unwrap();

	if let Some(cc_blob) = detect_cc_blob() {
		fdt = fdt.efi_sev_snp_cc_blob(cc_blob).unwrap()
	};

	if let Some(kernel_args) = kernel_args {
		fdt = fdt.bootargs(kernel_args.hermit_args).unwrap();
	} else if let Some(bootargs) = esp.read_bootargs() {
		fdt = fdt.bootargs(bootargs).unwrap();
	}

	allocator::exit_boot_services();
	let mut memory_map = unsafe { boot::exit_boot_services(None) };

	let fdt = fdt.memory_map(&mut memory_map).unwrap().finish().unwrap();

	unsafe { boot_kernel(kernel_info, fdt) }
}

pub unsafe fn boot_kernel(kernel_info: LoadedKernel, fdt: Vec<u8>) -> ! {
	let LoadedKernel {
		load_info,
		entry_point,
	} = kernel_info;

	let device_tree =
		DeviceTreeAddress::new(u64::try_from(fdt.leak().as_ptr().expose_provenance()).unwrap());

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
	let entry = ptr::with_exposed_provenance(entry_point.try_into().unwrap());
	let stack = ptr::with_exposed_provenance_mut(stack);
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
			.find(|entry| entry.guid == ConfigTableEntry::ACPI2_GUID)
		{
			(entry.address, 2)
		} else {
			let entry = config_table
				.iter()
				.find(|entry| entry.guid == ConfigTableEntry::ACPI_GUID)
				.unwrap();
			(entry.address, 1)
		};
		info!("Found ACPI {version} RSDP at {rsdp:p}");
		rsdp
	})
}

pub struct BootPartition {
	fs: FileSystem,
}

impl BootPartition {
	pub fn new() -> uefi::Result<Self> {
		let image_handle = boot::image_handle();
		let fs = boot::get_image_file_system(image_handle)?;
		let fs = FileSystem::new(fs);
		Ok(Self { fs })
	}

	pub fn read_app(&mut self) -> Vec<u8> {
		self.read_app_at(cstr16!(r"\EFI\hermit\hermit-app"))
			.or_else(|| self.read_app_at(cstr16!(r"\EFI\BOOT\hermit-app")))
			.unwrap()
	}

	pub fn read_bootargs(&mut self) -> Option<String> {
		self.read_bootargs_at(cstr16!(r"\EFI\hermit\hermit-bootargs"))
			.or_else(|| self.read_bootargs_at(cstr16!(r"\EFI\BOOT\hermit-bootargs")))
	}

	fn read_app_at<P: AsRef<Path>>(&mut self, path: P) -> Option<Vec<u8>> {
		fn inner(fs: &mut FileSystem, path: &Path) -> Option<Vec<u8>> {
			match fs.read(path) {
				Ok(data) => {
					let len = data.len();
					info!("Read Hermit application from {path} (size = {len} B)");
					Some(data)
				}
				Err(err) => {
					let err = anyhow!(err);
					error!("Could not read Hermit application: {err:?}");
					None
				}
			}
		}

		inner(&mut self.fs, path.as_ref())
	}

	fn read_bootargs_at<P: AsRef<Path>>(&mut self, path: P) -> Option<String> {
		fn inner(fs: &mut FileSystem, path: &Path) -> Option<String> {
			match fs.read_to_string(path) {
				Ok(bootargs) => {
					info!("Read Hermit bootargs from from {path}: {bootargs}");
					Some(bootargs)
				}
				Err(fs::Error::Io(err)) if err.uefi_error.status() == Status::NOT_FOUND => {
					info!("Hermit bootargs not found at {path}");
					None
				}
				Err(err) => {
					let err = anyhow!(err);
					error!("Could not read Hermit bootargs: {err:#}");
					None
				}
			}
		}

		inner(&mut self.fs, path.as_ref())
	}
}

/// Try to locate an AMD SEV-SNP confidential computing blob, and returns its address if found.
/// This function is only relevant on AMD SEV-SNP guests, and will do mostly nothing otherwise.
fn detect_cc_blob() -> Option<u64> {
	/// EFI SEV-SNP Confidential Computing Blob configuration table.
	/// See <https://docs.amd.com/v/u/en-US/56421>.
	const CC_BLOB_GUID: Guid = guid!("067b1f5f-cf26-44c5-8554-93d777912d42");

	system::with_config_table(|config_table| {
		config_table
			.iter()
			.find(|entry| entry.guid == CC_BLOB_GUID)
			.map(|entry| {
				info!(
					"EFI SEV-SNP Confidential Computing Blob found at {:p}",
					entry.address
				);
				u64::try_from(entry.address.addr()).unwrap()
			})
	})
}

/// Reads arguments passed when using Kernel Direct Boot (`-kernel -initrd` arguments with UEFI
/// support)
#[derive(Debug)]
struct KernelArguments {
	/// Arguments that should be forwarded to Hermit
	hermit_args: String,

	/// Image path, overriding default option
	initrd_path: Option<CString16>,
}

impl KernelArguments {
	pub fn new() -> uefi::Result<Option<Self>> {
		let image_handle = boot::image_handle();
		let loaded_image = open_protocol_exclusive::<LoadedImage>(image_handle)?;
		let Some(raw_options) = loaded_image.load_options_as_cstr16().ok() else {
			return Ok(None);
		};

		let raw_options: String = raw_options.into();
		let args = if let Some(rest) = raw_options.strip_prefix("initrd=") {
			let (initrd, rest) = rest.split_once(' ').unwrap();
			Self {
				hermit_args: rest.into(),
				initrd_path: Some(initrd.try_into().unwrap()),
			}
		} else {
			Self {
				hermit_args: raw_options,
				initrd_path: None,
			}
		};

		info!("Read QEMU kernel arguments: {args:?}");

		Ok(Some(args))
	}
}
