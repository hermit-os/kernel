use alloc::vec::Vec;
use core::ptr::NonNull;

use align_address::Align;
use arm_gic::{IntId, Trigger};
#[cfg(any(feature = "virtio-console", feature = "virtio-fs"))]
use hermit_sync::InterruptTicketMutex;
use hermit_sync::without_interrupts;
use virtio::mmio::{DeviceRegisters, DeviceRegistersVolatileFieldAccess};
use volatile::VolatileRef;

use crate::arch::aarch64::kernel::interrupts::GIC;
use crate::arch::aarch64::mm::paging::{self, PageSize};
#[cfg(feature = "virtio-console")]
use crate::console::IoDevice;
#[cfg(feature = "virtio-console")]
use crate::drivers::console::VirtioConsoleDriver;
#[cfg(feature = "virtio-console")]
use crate::drivers::console::VirtioUART;
#[cfg(feature = "virtio-fs")]
use crate::drivers::fs::VirtioFsDriver;
#[cfg(feature = "virtio-net")]
use crate::drivers::net::virtio::VirtioNetDriver;
use crate::drivers::virtio::transport::mmio as mmio_virtio;
#[cfg(any(
	feature = "virtio-console",
	feature = "virtio-fs",
	feature = "virtio-net",
))]
use crate::drivers::virtio::transport::mmio::VirtioDriver;
#[cfg(feature = "virtio-net")]
use crate::executor::device::NETWORK_DEVICE;
use crate::init_cell::InitCell;
use crate::mm::PhysAddr;

pub(crate) static MMIO_DRIVERS: InitCell<Vec<MmioDriver>> = InitCell::new(Vec::new());

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

pub fn init_drivers() {
	without_interrupts(|| {
		let Some(fdt) = crate::env::fdt() else {
			error!("No device tree found, cannot initialize MMIO drivers");
			return;
		};

		for node in fdt.find_all_nodes("/virtio_mmio") {
			let Some(compatible) = node.compatible() else {
				continue;
			};

			for i in compatible.all() {
				if i == "virtio,mmio" {
					let virtio_region = node
						.reg()
						.expect("reg property for virtio mmio not found in FDT")
						.next()
						.unwrap();
					let mut irq = 0;
					let mut irqtype = 0;
					let mut irqflags = 0;

					for prop in node.properties() {
						if prop.name == "interrupts" {
							irqtype = u32::from_be_bytes(prop.value[0..4].try_into().unwrap());
							irq = u32::from_be_bytes(prop.value[4..8].try_into().unwrap());
							irqflags = u32::from_be_bytes(prop.value[8..12].try_into().unwrap());
							break;
						}
					}

					let virtio_region_start =
						PhysAddr::from(virtio_region.starting_address.expose_provenance());

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
					let cpu_id: usize = 0;

					if id == virtio::Id::Reserved {
						continue;
					}

					debug!(
						"Found {id:?} card at {mmio:p}, irq: {irq}, type: {irqtype}, flags: {irqflags}"
					);

					let drv = match mmio_virtio::init_device(mmio, irq.try_into().unwrap()) {
						Ok(drv) => drv,
						Err(err) => {
							error!("{err}");
							continue;
						}
					};

					let mut gic = GIC.lock();
					let Some(gic) = gic.as_mut() else {
						error!("No GIC found");
						continue;
					};

					// enable timer interrupt
					let virtio_irqid = if irqtype == 1 {
						IntId::ppi(irq)
					} else if irqtype == 0 {
						IntId::spi(irq)
					} else {
						panic!("Invalid interrupt type");
					};
					gic.set_interrupt_priority(virtio_irqid, Some(cpu_id), 0x00);
					if (irqflags & 0xf) == 4 || (irqflags & 0xf) == 8 {
						gic.set_trigger(virtio_irqid, Some(cpu_id), Trigger::Level);
					} else if (irqflags & 0xf) == 2 || (irqflags & 0xf) == 1 {
						gic.set_trigger(virtio_irqid, Some(cpu_id), Trigger::Edge);
					} else {
						panic!("Invalid interrupt level!");
					}
					gic.enable_interrupt(virtio_irqid, Some(cpu_id), true);

					match drv {
						#[cfg(feature = "virtio-console")]
						VirtioDriver::Console(drv) => register_driver(MmioDriver::VirtioConsole(
							InterruptTicketMutex::new(*drv),
						)),
						#[cfg(feature = "virtio-fs")]
						VirtioDriver::FileSystem(drv) => register_driver(MmioDriver::VirtioFs(
							hermit_sync::InterruptTicketMutex::new(*drv),
						)),
						#[cfg(feature = "virtio-net")]
						VirtioDriver::Net(drv) => *NETWORK_DEVICE.lock() = Some(*drv),
					}
				}
			}
		}
	});

	MMIO_DRIVERS.finalize();

	#[cfg(feature = "virtio-console")]
	{
		if get_console_driver().is_some() {
			info!("Switch to virtio console");
			crate::console::CONSOLE
				.lock()
				.replace_device(IoDevice::Virtio(VirtioUART::new()));
		}
	}
}
