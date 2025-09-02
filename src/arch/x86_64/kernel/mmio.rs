#![allow(dead_code)]
#![allow(unused_imports)]

use alloc::string::String;
use alloc::vec::Vec;
use core::ptr::NonNull;
use core::{ptr, str};

use align_address::Align;
use free_list::{PageLayout, PageRange};
use hermit_sync::{InterruptTicketMutex, without_interrupts};
use memory_addresses::{PhysAddr, VirtAddr};
use virtio::mmio::{DeviceRegisters, DeviceRegistersVolatileFieldAccess};
use volatile::VolatileRef;

use crate::arch::x86_64::mm::paging;
use crate::arch::x86_64::mm::paging::{
	BasePageSize, PageSize, PageTableEntryFlags, PageTableEntryFlagsExt,
};
#[cfg(feature = "console")]
use crate::drivers::console::VirtioConsoleDriver;
#[cfg(feature = "virtio-net")]
use crate::drivers::net::virtio::VirtioNetDriver;
use crate::drivers::virtio::transport::mmio as mmio_virtio;
use crate::drivers::virtio::transport::mmio::VirtioDriver;
use crate::env;
#[cfg(all(
	any(feature = "rtl8139", feature = "virtio-net"),
	any(feature = "tcp", feature = "udp"),
))]
use crate::executor::device::NETWORK_DEVICE;
use crate::init_cell::InitCell;
use crate::mm::physicalmem::PHYSICAL_FREE_LIST;
use crate::mm::virtualmem::KERNEL_FREE_LIST;

pub const MAGIC_VALUE: u32 = 0x7472_6976;

pub const MMIO_START: usize = 0x0000_0000_feb0_0000;
pub const MMIO_END: usize = 0x0000_0000_feb0_ffff;
const IRQ_NUMBER: u8 = 44 - 32;

static MMIO_DRIVERS: InitCell<Vec<MmioDriver>> = InitCell::new(Vec::new());

pub(crate) enum MmioDriver {
	#[cfg(feature = "console")]
	VirtioConsole(InterruptTicketMutex<VirtioConsoleDriver>),
}

impl MmioDriver {
	#[cfg(feature = "console")]
	fn get_console_driver(&self) -> Option<&InterruptTicketMutex<VirtioConsoleDriver>> {
		match self {
			Self::VirtioConsole(drv) => Some(drv),
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

	// We found a MMIO-device (whose 512-bit address in this structure).
	trace!("Found a MMIO-device at {mmio:p}");

	// Verify the device-ID to find the network card
	let id = mmio.as_ptr().device_id().read();

	if id != virtio::Id::Net {
		trace!("It's not a network card at {mmio:p}");
		return None;
	}

	Some(mmio)
}

fn check_linux_args(
	linux_mmio: &'static [String],
) -> Result<(VolatileRef<'static, DeviceRegisters>, u8), &'static str> {
	let layout = PageLayout::from_size(BasePageSize::SIZE as usize).unwrap();
	let page_range = KERNEL_FREE_LIST.lock().allocate(layout).unwrap();
	let virtual_address = VirtAddr::from(page_range.start());

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

					PHYSICAL_FREE_LIST
						.lock()
						.allocate_at(frame_range)
						.unwrap_err();
				}

				return Ok((mmio, irq));
			}
			_ => {
				warn!("Invalid prefix in {arg}");
			}
		}
	}

	// frees obsolete virtual memory region for MMIO devices
	let range =
		PageRange::from_start_len(virtual_address.as_usize(), BasePageSize::SIZE as usize).unwrap();
	unsafe {
		KERNEL_FREE_LIST.lock().deallocate(range).unwrap();
	}

	Err("Network card not found!")
}

fn guess_device() -> Result<(VolatileRef<'static, DeviceRegisters>, u8), &'static str> {
	// Trigger page mapping in the first iteration!
	let mut current_page = 0;
	let layout = PageLayout::from_size(BasePageSize::SIZE as usize).unwrap();
	let page_range = KERNEL_FREE_LIST.lock().allocate(layout).unwrap();
	let virtual_address = VirtAddr::from(page_range.start());

	// Look for the device-ID in all possible 64-byte aligned addresses within this range.
	for current_address in (MMIO_START..MMIO_END).step_by(512) {
		trace!("try to detect MMIO device at physical address {current_address:#X}");
		// Have we crossed a page boundary in the last iteration?
		// info!("before the {}. paging", current_page);
		if current_address / BasePageSize::SIZE as usize > current_page {
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

		info!("Found network card at {mmio:p}");

		if cfg!(debug_assertions) {
			let len = usize::try_from(BasePageSize::SIZE).unwrap();
			let start = current_address.align_down(len);
			let frame_range = PageRange::from_start_len(start, len).unwrap();

			PHYSICAL_FREE_LIST
				.lock()
				.allocate_at(frame_range)
				.unwrap_err();
		}

		return Ok((mmio, IRQ_NUMBER));
	}

	// frees obsolete virtual memory region for MMIO devices
	let range =
		PageRange::from_start_len(virtual_address.as_usize(), BasePageSize::SIZE as usize).unwrap();
	unsafe {
		KERNEL_FREE_LIST.lock().deallocate(range).unwrap();
	}

	Err("Network card not found!")
}

/// Tries to find the network device within the specified address range.
/// Returns a reference to it within the Ok() if successful or an Err() on failure.
fn detect_network() -> Result<(VolatileRef<'static, DeviceRegisters>, u8), &'static str> {
	let linux_mmio = env::mmio();

	if linux_mmio.is_empty() {
		guess_device()
	} else {
		check_linux_args(linux_mmio)
	}
}

pub(crate) fn register_driver(drv: MmioDriver) {
	MMIO_DRIVERS.with(|mmio_drivers| mmio_drivers.unwrap().push(drv));
}

#[cfg(feature = "virtio-net")]
pub(crate) type NetworkDevice = VirtioNetDriver;

#[cfg(feature = "console")]
pub(crate) fn get_console_driver() -> Option<&'static InterruptTicketMutex<VirtioConsoleDriver>> {
	MMIO_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_console_driver())
}

pub(crate) fn init_drivers() {
	// virtio: MMIO Device Discovery
	without_interrupts(|| {
		#[cfg(feature = "virtio-net")]
		if let Ok((mmio, irq)) = detect_network() {
			warn!("Found MMIO device, but we guess the interrupt number {irq}!");
			match mmio_virtio::init_device(mmio, irq) {
				Ok(VirtioDriver::Network(drv)) => {
					*NETWORK_DEVICE.lock() = Some(drv);
				}
				#[cfg(feature = "console")]
				Ok(VirtioDriver::Console(_)) => unreachable!(),
				Err(err) => error!("Could not initialize virtio-mmio device: {err}"),
			}
		} else {
			warn!("Unable to find mmio device");
		}

		MMIO_DRIVERS.finalize();
	});
}
