use alloc::vec::Vec;
use core::ptr::NonNull;

use align_address::Align;
use hermit_sync::{InterruptTicketMutex, without_interrupts};
use virtio::mmio::{DeviceRegisters, DeviceRegistersVolatileFieldAccess};
use volatile::VolatileRef;

use crate::arch::aarch64::mm::paging::{self, PageSize};
use crate::drivers::net::virtio::VirtioNetDriver;
use crate::drivers::virtio::transport::mmio::{self as mmio_virtio, VirtioDriver};
use crate::init_cell::InitCell;
use crate::mm::PhysAddr;

pub(crate) static MMIO_DRIVERS: InitCell<Vec<MmioDriver>> = InitCell::new(Vec::new());

pub(crate) enum MmioDriver {
	#[cfg(any(feature = "tcp", feature = "udp"))]
	VirtioNet(InterruptTicketMutex<VirtioNetDriver>),
}

impl MmioDriver {
	#[cfg(any(feature = "tcp", feature = "udp"))]
	fn get_network_driver(&self) -> Option<&InterruptTicketMutex<VirtioNetDriver>> {
		match self {
			Self::VirtioNet(drv) => Some(drv),
		}
	}
}

pub(crate) fn register_driver(drv: MmioDriver) {
	MMIO_DRIVERS.with(|mmio_drivers| mmio_drivers.unwrap().push(drv));
}

#[cfg(any(feature = "tcp", feature = "udp"))]
pub(crate) fn get_network_driver() -> Option<&'static InterruptTicketMutex<VirtioNetDriver>> {
	MMIO_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_network_driver())
}

pub fn init_drivers() {
	without_interrupts(|| {
		if let Some(fdt) = crate::env::fdt() {
			for node in fdt.find_all_nodes("/virtio_mmio") {
				if let Some(compatible) = node.compatible() {
					for i in compatible.all() {
						if i == "virtio,mmio" {
							let virtio_region = node
								.reg()
								.expect("reg property for virtio mmio not found in FDT")
								.next()
								.unwrap();
							let mut irq = 0;

							for prop in node.properties() {
								if prop.name == "interrupts" {
									irq = u32::from_be_bytes(prop.value[4..8].try_into().unwrap())
										.try_into()
										.unwrap();
									break;
								}
							}

							let virtio_region_start =
								PhysAddr::new(virtio_region.starting_address as u64);

							assert!(
								virtio_region.size.unwrap()
									< usize::try_from(paging::BasePageSize::SIZE).unwrap()
							);
							paging::identity_map::<paging::BasePageSize>(
								virtio_region_start.align_down(paging::BasePageSize::SIZE),
							);

							// Verify the first register value to find out if this is really an MMIO magic-value.
							let ptr = virtio_region.starting_address as *mut DeviceRegisters;
							let mmio = unsafe { VolatileRef::new(NonNull::new(ptr).unwrap()) };

							let magic = mmio.as_ptr().magic_value().read().to_ne();
							let version = mmio.as_ptr().version().read().to_ne();

							const MMIO_MAGIC_VALUE: u32 = 0x7472_6976;
							if magic != MMIO_MAGIC_VALUE {
								error!("It's not a MMIO-device at {mmio:p}");
							}

							if version != 2 {
								warn!("Found a legacy device, which isn't supported");
							}

							// We found a MMIO-device (whose 512-bit address in this structure).
							trace!("Found a MMIO-device at {mmio:p}");

							// Verify the device-ID to find the network card
							let id = mmio.as_ptr().device_id().read();

							#[cfg(any(feature = "tcp", feature = "udp"))]
							if id == virtio::Id::Net {
								trace!("Found network card at {mmio:p}, irq: {irq}");
								if let Ok(VirtioDriver::Network(drv)) =
									mmio_virtio::init_device(mmio, irq.try_into().unwrap())
								{
									register_driver(MmioDriver::VirtioNet(
										hermit_sync::InterruptTicketMutex::new(drv),
									));
								}
							}
						}
					}
				}
			}
		} else {
			error!("No device tree found, cannot initialize MMIO drivers");
		}
	});

	MMIO_DRIVERS.finalize();
}
