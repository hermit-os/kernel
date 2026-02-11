use alloc::string::String;
use alloc::vec::Vec;
use core::ptr::NonNull;
use core::{ptr, str};

use align_address::Align;
use free_list::{PageLayout, PageRange};
#[cfg(any(feature = "virtio-console", feature = "virtio-fs"))]
use hermit_sync::InterruptTicketMutex;
use hermit_sync::without_interrupts;
use memory_addresses::{PhysAddr, VirtAddr};
use virtio::mmio::{DeviceRegisters, DeviceRegistersVolatileFieldAccess};
use volatile::VolatileRef;

use crate::arch::x86_64::mm::paging;
use crate::arch::x86_64::mm::paging::{
	BasePageSize, PageSize, PageTableEntryFlags, PageTableEntryFlagsExt,
};
#[cfg(feature = "virtio-console")]
use crate::drivers::console::VirtioConsoleDriver;
#[cfg(feature = "virtio-fs")]
use crate::drivers::fs::VirtioFsDriver;
#[cfg(feature = "virtio-net")]
use crate::drivers::net::virtio::VirtioNetDriver;
use crate::drivers::virtio::transport::mmio as mmio_virtio;
#[cfg(any(
	feature = "virtio-console",
	feature = "virtio-fs",
	feature = "virtio-net"
))]
use crate::drivers::virtio::transport::mmio::VirtioDriver;
use crate::env;
#[cfg(any(feature = "rtl8139", feature = "virtio-net"))]
use crate::executor::device::NETWORK_DEVICE;
use crate::init_cell::InitCell;
use crate::mm::{FrameAlloc, PageBox, PageRangeAllocator};

pub const MAGIC_VALUE: u32 = 0x7472_6976;

pub const MMIO_START: usize = 0x0000_0000_feb0_0000;
pub const MMIO_END: usize = 0x0000_0000_feb0_ffff;
const IRQ_NUMBER: u8 = 44 - 32;

static MMIO_DRIVERS: InitCell<Vec<MmioDriver>> = InitCell::new(Vec::new());

pub(crate) enum MmioDriver {
	#[cfg(feature = "virtio-console")]
	VirtioConsole(InterruptTicketMutex<VirtioConsoleDriver>),
	#[cfg(feature = "virtio-fs")]
	VirtioFs(InterruptTicketMutex<VirtioFsDriver>),
}

impl MmioDriver {
	#[cfg(feature = "virtio-console")]
	fn get_console_driver(&self) -> Option<&InterruptTicketMutex<VirtioConsoleDriver>> {
		match self {
			Self::VirtioConsole(drv) => Some(drv),
		}
	}

	#[cfg(feature = "virtio-fs")]
	fn get_filesystem_driver(&self) -> Option<&InterruptTicketMutex<VirtioFsDriver>> {
		match self {
			Self::VirtioFs(drv) => Some(drv),
		}
	}
}

unsafe fn check_ptr(ptr: *mut u8) -> Option<VolatileRef<'static, DeviceRegisters>> {
	// Verify the first register value to find out if this is really an MMIO magic-value.
	let mmio = unsafe { VolatileRef::new(NonNull::new(ptr.cast::<DeviceRegisters>()).unwrap()) };

	let magic = mmio.as_ptr().magic_value().read().to_ne();
	let version = mmio.as_ptr().version().read().to_ne();

	if magic != MAGIC_VALUE {
		trace!("It's not a MMIO-device at {mmio:p}");
		return None;
	}

	if version != 2 {
		trace!("Found a legacy device, which isn't supported");
		return None;
	}

	let id = mmio.as_ptr().device_id().read();

	if id == virtio::Id::Reserved {
		return None;
	}

	info!("Found Virtio {id:?} device: {mmio:p}");

	Some(mmio)
}

fn check_linux_args(
	linux_mmio: &'static [String],
) -> Vec<(VolatileRef<'static, DeviceRegisters>, u8)> {
	let layout = PageLayout::from_size(BasePageSize::SIZE as usize).unwrap();
	let page_range = PageBox::new(layout).unwrap();
	let virtual_address = VirtAddr::from(page_range.start());

	let mut devices = vec![];
	for arg in linux_mmio {
		trace!("check linux parameter: {arg}");

		match arg.trim().trim_matches(char::from(0)).strip_prefix("4K@") {
			Some(arg) => {
				let v: Vec<&str> = arg.trim().split(':').collect();
				let without_prefix = v[0].trim_start_matches("0x");
				let current_address = usize::from_str_radix(without_prefix, 16).unwrap();
				let irq: u8 = v[1].parse::<u8>().unwrap();

				trace!("try to detect MMIO device at physical address {current_address:#X}");

				let mut flags = PageTableEntryFlags::empty();
				flags.normal().writable();
				paging::map::<BasePageSize>(
					virtual_address,
					PhysAddr::from(current_address.align_down(BasePageSize::SIZE as usize)),
					1,
					flags,
				);

				let addr = virtual_address.as_usize()
					| (current_address & (BasePageSize::SIZE as usize - 1));
				let ptr = ptr::with_exposed_provenance_mut(addr);
				let Some(mmio) = (unsafe { check_ptr(ptr) }) else {
					continue;
				};

				if cfg!(debug_assertions) {
					let len = usize::try_from(BasePageSize::SIZE).unwrap();
					let start = current_address.align_down(len);
					let frame_range = PageRange::from_start_len(start, len).unwrap();

					FrameAlloc::allocate_at(frame_range).unwrap_err();
				}

				devices.push((mmio, irq));
			}
			_ => {
				warn!("Invalid prefix in {arg}");
			}
		}
	}

	devices
}

fn guess_device() -> Vec<(VolatileRef<'static, DeviceRegisters>, u8)> {
	// Trigger page mapping in the first iteration!
	let mut current_page = 0;
	let layout = PageLayout::from_size(BasePageSize::SIZE as usize).unwrap();
	let page_range = PageBox::new(layout).unwrap();
	let virtual_address = VirtAddr::from(page_range.start());

	// Look for the device-ID in all possible 64-byte aligned addresses within this range.
	let mut devices = vec![];
	for current_address in (MMIO_START..MMIO_END).step_by(512) {
		trace!("try to detect MMIO device at physical address {current_address:#X}");
		// Have we crossed a page boundary in the last iteration?
		// info!("before the {}. paging", current_page);
		if current_address / BasePageSize::SIZE as usize > current_page {
			if !devices.is_empty() {
				return devices;
			}

			let mut flags = PageTableEntryFlags::empty();
			flags.normal().writable();
			paging::map::<BasePageSize>(
				virtual_address,
				PhysAddr::from(current_address.align_down(BasePageSize::SIZE as usize)),
				1,
				flags,
			);

			current_page = current_address / BasePageSize::SIZE as usize;
		}

		let addr =
			virtual_address.as_usize() | (current_address & (BasePageSize::SIZE as usize - 1));
		let ptr = ptr::with_exposed_provenance_mut(addr);
		let Some(mmio) = (unsafe { check_ptr(ptr) }) else {
			continue;
		};

		if cfg!(debug_assertions) {
			let len = usize::try_from(BasePageSize::SIZE).unwrap();
			let start = current_address.align_down(len);
			let frame_range = PageRange::from_start_len(start, len).unwrap();

			FrameAlloc::allocate_at(frame_range).unwrap_err();
		}

		devices.push((mmio, IRQ_NUMBER));
	}

	devices
}

fn detect_devices() -> Vec<(VolatileRef<'static, DeviceRegisters>, u8)> {
	let linux_mmio = env::mmio();

	if linux_mmio.is_empty() {
		guess_device()
	} else {
		check_linux_args(linux_mmio)
	}
}

#[cfg(any(feature = "virtio-console", feature = "virtio-fs"))]
pub(crate) fn register_driver(drv: MmioDriver) {
	MMIO_DRIVERS.with(|mmio_drivers| mmio_drivers.unwrap().push(drv));
}

#[cfg(feature = "virtio-net")]
pub(crate) type NetworkDevice = VirtioNetDriver;

#[cfg(feature = "virtio-console")]
pub(crate) fn get_console_driver() -> Option<&'static InterruptTicketMutex<VirtioConsoleDriver>> {
	MMIO_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_console_driver())
}

#[cfg(feature = "virtio-fs")]
pub(crate) fn get_filesystem_driver() -> Option<&'static InterruptTicketMutex<VirtioFsDriver>> {
	MMIO_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_filesystem_driver())
}

pub(crate) fn init_drivers() {
	without_interrupts(|| {
		let devices = detect_devices();

		for (mmio, irq) in devices {
			match mmio_virtio::init_device(mmio, irq) {
				#[cfg(feature = "virtio-console")]
				Ok(VirtioDriver::Console(drv)) => {
					register_driver(MmioDriver::VirtioConsole(InterruptTicketMutex::new(*drv)));
				}
				#[cfg(feature = "virtio-fs")]
				Ok(VirtioDriver::Fs(drv)) => {
					register_driver(MmioDriver::VirtioFs(InterruptTicketMutex::new(*drv)));
				}
				#[cfg(feature = "virtio-net")]
				Ok(VirtioDriver::Net(drv)) => {
					*NETWORK_DEVICE.lock() = Some(*drv);
				}
				Err(err) => error!("Could not initialize virtio-mmio device: {err}"),
			}
		}

		MMIO_DRIVERS.finalize();

		#[cfg(feature = "virtio-console")]
		if get_console_driver().is_some() {
			use crate::console::IoDevice;
			use crate::drivers::console::VirtioUART;

			info!("Switch to virtio console");
			crate::console::CONSOLE
				.lock()
				.replace_device(IoDevice::Virtio(VirtioUART::new()));
		}
	});
}
