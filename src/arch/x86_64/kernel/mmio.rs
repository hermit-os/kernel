use crate::collections::irqsave;

use crate::arch::x86_64::mm::paging;
use crate::arch::x86_64::mm::paging::{
	BasePageSize, PageSize, PageTableEntryFlags, PageTableEntryFlagsExt,
};
use crate::arch::x86_64::mm::PhysAddr;
use crate::drivers::net::virtio_net::VirtioNetDriver;
use crate::drivers::net::NetworkInterface;
use crate::drivers::virtio::transport::mmio as mmio_virtio;
use crate::drivers::virtio::transport::mmio::{DevId, MmioRegisterLayout, VirtioDriver};
use crate::synch::spinlock::SpinlockIrqSave;
use alloc::vec::Vec;
use core::str;

pub const MAGIC_VALUE: u32 = 0x74726976;

pub const MMIO_START: usize = 0x00000000c0000000;
pub const MMIO_END: usize = 0x00000000c0000fff;
const IRQ_NUMBER: u32 = 12;

static mut MMIO_DRIVERS: Vec<MmioDriver> = Vec::new();

pub enum MmioDriver {
	VirtioNet(SpinlockIrqSave<VirtioNetDriver>),
}

impl MmioDriver {
	#[allow(unreachable_patterns)]
	fn get_network_driver(&self) -> Option<&SpinlockIrqSave<dyn NetworkInterface>> {
		match self {
			Self::VirtioNet(drv) => Some(drv),
			_ => None,
		}
	}
}

/// Tries to find the network device within the specified address range.
/// Returns a reference to it within the Ok() if successful or an Err() on failure.
pub fn detect_network() -> Result<&'static mut MmioRegisterLayout, &'static str> {
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
				PhysAddr::from(align_down!(current_address, BasePageSize::SIZE as usize)),
				1,
				flags,
			);

			current_page = current_address / BasePageSize::SIZE as usize;
		}

		// Verify the first register value to find out if this is really an MMIO magic-value.
		let mmio = unsafe {
			&mut *((virtual_address.as_usize()
				| (current_address & (BasePageSize::SIZE as usize - 1)))
				as *mut MmioRegisterLayout)
		};

		let magic = mmio.get_magic_value();
		let version = mmio.get_version();

		if magic != MAGIC_VALUE {
			trace!("It's not a MMIO-device at {:#X}", mmio as *const _ as usize);
			continue;
		}

		if version != 2 {
			trace!("Found a legacy device, which isn't supported");
			continue;
		}

		// We found a MMIO-device (whose 512-bit address in this structure).
		trace!("Found a MMIO-device at {:#X}", mmio as *const _ as usize);

		// Verify the device-ID to find the network card
		let id = mmio.get_device_id();

		if id != DevId::VIRTIO_DEV_ID_NET {
			trace!(
				"It's not a network card at {:#X}",
				mmio as *const _ as usize
			);
			continue;
		}

		info!("Found network card at {:#X}", mmio as *const _ as usize);

		crate::arch::mm::physicalmem::reserve(
			PhysAddr::from(align_down!(current_address, BasePageSize::SIZE as usize)),
			BasePageSize::SIZE as usize,
		);

		//mmio.print_information();

		return Ok(mmio);
	}

	// frees obsolete virtual memory region for MMIO devices
	crate::arch::mm::virtualmem::deallocate(virtual_address, BasePageSize::SIZE as usize);

	Err("Network card not found!")
}

pub fn register_driver(drv: MmioDriver) {
	unsafe {
		MMIO_DRIVERS.push(drv);
	}
}

pub fn get_network_driver() -> Option<&'static SpinlockIrqSave<dyn NetworkInterface>> {
	unsafe { MMIO_DRIVERS.iter().find_map(|drv| drv.get_network_driver()) }
}

pub fn init_drivers() {
	// virtio: MMIO Device Discovery
	irqsave(|| {
		if let Ok(mmio) = detect_network() {
			warn!(
				"Found MMIO device, but we guess the interrupt number {}!",
				IRQ_NUMBER
			);
			if let Ok(VirtioDriver::Network(drv)) = mmio_virtio::init_device(mmio, IRQ_NUMBER) {
				register_driver(MmioDriver::VirtioNet(SpinlockIrqSave::new(drv)))
			}
		} else {
			warn!("Unable to find mmio device");
		}
	});
}
