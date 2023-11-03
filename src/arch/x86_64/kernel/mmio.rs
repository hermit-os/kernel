use alloc::string::String;
use alloc::vec::Vec;
use core::{ptr, str};

use align_address::Align;
use hermit_sync::{without_interrupts, InterruptTicketMutex};

use crate::arch::x86_64::mm::paging::{
	BasePageSize, PageSize, PageTableEntryFlags, PageTableEntryFlagsExt,
};
use crate::arch::x86_64::mm::{paging, PhysAddr};
use crate::drivers::net::virtio_net::VirtioNetDriver;
use crate::drivers::virtio::transport::mmio as mmio_virtio;
use crate::drivers::virtio::transport::mmio::{DevId, MmioRegisterLayout, VirtioDriver};
use crate::env;

pub const MAGIC_VALUE: u32 = 0x74726976;

pub const MMIO_START: usize = 0x00000000c0000000;
pub const MMIO_END: usize = 0x00000000c0000fff;
const IRQ_NUMBER: u8 = 12;

static mut MMIO_DRIVERS: Vec<MmioDriver> = Vec::new();

pub(crate) enum MmioDriver {
	VirtioNet(InterruptTicketMutex<VirtioNetDriver>),
}

impl MmioDriver {
	#[allow(unreachable_patterns)]
	fn get_network_driver(&self) -> Option<&InterruptTicketMutex<VirtioNetDriver>> {
		match self {
			Self::VirtioNet(drv) => Some(drv),
			_ => None,
		}
	}
}

fn check_linux_args(
	linux_mmio: &'static [String],
) -> Result<(&'static mut MmioRegisterLayout, u8), &'static str> {
	let virtual_address =
		crate::arch::mm::virtualmem::allocate(BasePageSize::SIZE as usize).unwrap();

	for arg in linux_mmio {
		trace!("check linux parameter: {}", arg);

		match arg.trim().trim_matches(char::from(0)).strip_prefix("4K@") {
			Some(arg) => {
				let v: Vec<&str> = arg.trim().split(':').collect();
				let without_prefix = v[0].trim_start_matches("0x");
				let current_address = usize::from_str_radix(without_prefix, 16).unwrap();
				let irq: u8 = v[1].parse::<u8>().unwrap();

				trace!(
					"try to detect MMIO device at physical address {:#X}",
					current_address
				);

				let mut flags = PageTableEntryFlags::empty();
				flags.normal().writable();
				paging::map::<BasePageSize>(
					virtual_address,
					PhysAddr::from(current_address.align_down(BasePageSize::SIZE as usize)),
					1,
					flags,
				);

				// Verify the first register value to find out if this is really an MMIO magic-value.
				let mmio = unsafe {
					&mut *(ptr::from_exposed_addr_mut::<MmioRegisterLayout>(
						virtual_address.as_usize()
							| (current_address & (BasePageSize::SIZE as usize - 1)),
					))
				};

				let magic = mmio.get_magic_value();
				let version = mmio.get_version();

				if magic != MAGIC_VALUE {
					trace!("It's not a MMIO-device at {mmio:p}");
					continue;
				}

				if version != 2 {
					trace!("Found a legacy device, which isn't supported");
					continue;
				}

				// We found a MMIO-device (whose 512-bit address in this structure).
				trace!("Found a MMIO-device at {mmio:p}");

				// Verify the device-ID to find the network card
				let id = mmio.get_device_id();

				if id != DevId::VIRTIO_DEV_ID_NET {
					trace!("It's not a network card at {mmio:p}");
					continue;
				}

				info!("Found network card at {mmio:p}, irq {}", irq);

				crate::arch::mm::physicalmem::reserve(
					PhysAddr::from(current_address.align_down(BasePageSize::SIZE as usize)),
					BasePageSize::SIZE as usize,
				);

				return Ok((mmio, irq));
			}
			_ => {
				warn!("Inavlid prefix in {}", arg);
			}
		}
	}

	// frees obsolete virtual memory region for MMIO devices
	crate::arch::mm::virtualmem::deallocate(virtual_address, BasePageSize::SIZE as usize);

	Err("Network card not found!")
}

fn guess_device() -> Result<(&'static mut MmioRegisterLayout, u8), &'static str> {
	// Trigger page mapping in the first iteration!
	let mut current_page = 0;
	let virtual_address =
		crate::arch::mm::virtualmem::allocate(BasePageSize::SIZE as usize).unwrap();

	// Look for the device-ID in all possible 64-byte aligned addresses within this range.
	for current_address in (MMIO_START..MMIO_END).step_by(512) {
		trace!(
			"try to detect MMIO device at physical address {:#X}",
			current_address
		);
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

		// Verify the first register value to find out if this is really an MMIO magic-value.
		let mmio = unsafe {
			&mut *(ptr::from_exposed_addr_mut::<MmioRegisterLayout>(
				virtual_address.as_usize() | (current_address & (BasePageSize::SIZE as usize - 1)),
			))
		};

		let magic = mmio.get_magic_value();
		let version = mmio.get_version();

		if magic != MAGIC_VALUE {
			trace!("It's not a MMIO-device at {mmio:p}");
			continue;
		}

		if version != 2 {
			trace!("Found a legacy device, which isn't supported");
			continue;
		}

		// We found a MMIO-device (whose 512-bit address in this structure).
		trace!("Found a MMIO-device at {mmio:p}");

		// Verify the device-ID to find the network card
		let id = mmio.get_device_id();

		if id != DevId::VIRTIO_DEV_ID_NET {
			trace!("It's not a network card at {mmio:p}");
			continue;
		}

		info!("Found network card at {mmio:p}");

		crate::arch::mm::physicalmem::reserve(
			PhysAddr::from(current_address.align_down(BasePageSize::SIZE as usize)),
			BasePageSize::SIZE as usize,
		);

		//mmio.print_information();

		return Ok((mmio, IRQ_NUMBER));
	}

	// frees obsolete virtual memory region for MMIO devices
	crate::arch::mm::virtualmem::deallocate(virtual_address, BasePageSize::SIZE as usize);

	Err("Network card not found!")
}

/// Tries to find the network device within the specified address range.
/// Returns a reference to it within the Ok() if successful or an Err() on failure.
fn detect_network() -> Result<(&'static mut MmioRegisterLayout, u8), &'static str> {
	let linux_mmio = env::mmio();

	if linux_mmio.len() > 0 {
		check_linux_args(linux_mmio)
	} else {
		guess_device()
	}
}

pub(crate) fn register_driver(drv: MmioDriver) {
	unsafe {
		MMIO_DRIVERS.push(drv);
	}
}

pub(crate) fn get_network_driver() -> Option<&'static InterruptTicketMutex<VirtioNetDriver>> {
	unsafe { MMIO_DRIVERS.iter().find_map(|drv| drv.get_network_driver()) }
}

pub(crate) fn init_drivers() {
	// virtio: MMIO Device Discovery
	without_interrupts(|| {
		if let Ok((mmio, irq)) = detect_network() {
			warn!(
				"Found MMIO device, but we guess the interrupt number {}!",
				irq
			);
			if let Ok(VirtioDriver::Network(drv)) = mmio_virtio::init_device(mmio, irq) {
				register_driver(MmioDriver::VirtioNet(InterruptTicketMutex::new(drv)))
			}
		} else {
			warn!("Unable to find mmio device");
		}
	});
}
