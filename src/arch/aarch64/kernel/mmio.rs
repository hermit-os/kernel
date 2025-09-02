use alloc::vec::Vec;
use core::ptr::NonNull;

use align_address::Align;
use arm_gic::{IntId, Trigger};
#[cfg(feature = "console")]
use hermit_sync::InterruptTicketMutex;
use hermit_sync::without_interrupts;
use virtio::mmio::{DeviceRegisters, DeviceRegistersVolatileFieldAccess};
use volatile::VolatileRef;

use crate::arch::aarch64::kernel::interrupts::GIC;
use crate::arch::aarch64::mm::paging::{self, PageSize};
#[cfg(feature = "console")]
use crate::console::IoDevice;
#[cfg(feature = "console")]
use crate::drivers::console::VirtioConsoleDriver;
#[cfg(feature = "console")]
use crate::drivers::console::VirtioUART;
#[cfg(any(feature = "tcp", feature = "udp"))]
use crate::drivers::net::virtio::VirtioNetDriver;
use crate::drivers::virtio::transport::mmio::{self as mmio_virtio, VirtioDriver};
#[cfg(all(feature = "virtio-net", any(feature = "tcp", feature = "udp")))]
use crate::executor::device::NETWORK_DEVICE;
use crate::init_cell::InitCell;
use crate::mm::PhysAddr;

pub(crate) static MMIO_DRIVERS: InitCell<Vec<MmioDriver>> = InitCell::new(Vec::new());

pub(crate) enum MmioDriver {
	#[cfg(feature = "console")]
	VirtioConsole(InterruptTicketMutex<VirtioConsoleDriver>),
}

impl MmioDriver {
	#[cfg(feature = "console")]
	fn get_console_driver(&self) -> Option<&InterruptTicketMutex<VirtioConsoleDriver>> {
		match self {
			Self::VirtioConsole(drv) => Some(drv),
			#[cfg(any(feature = "tcp", feature = "udp"))]
			_ => None,
		}
	}
}

#[cfg(feature = "console")]
pub(crate) fn register_driver(drv: MmioDriver) {
	MMIO_DRIVERS.with(|mmio_drivers| mmio_drivers.unwrap().push(drv));
}

#[cfg(any(feature = "tcp", feature = "udp"))]
pub(crate) type NetworkDevice = VirtioNetDriver;

#[cfg(feature = "console")]
pub(crate) fn get_console_driver() -> Option<&'static InterruptTicketMutex<VirtioConsoleDriver>> {
	MMIO_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_console_driver())
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
							let mut irqtype = 0;
							let mut irqflags = 0;

							for prop in node.properties() {
								if prop.name == "interrupts" {
									irqtype =
										u32::from_be_bytes(prop.value[0..4].try_into().unwrap());
									irq = u32::from_be_bytes(prop.value[4..8].try_into().unwrap());
									irqflags =
										u32::from_be_bytes(prop.value[8..12].try_into().unwrap());
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
							let cpu_id: usize = 0;

							match id {
								#[cfg(any(feature = "tcp", feature = "udp"))]
								virtio::Id::Net => {
									debug!(
										"Found network card at {mmio:p}, irq: {irq}, type: {irqtype}, flags: {irqflags}"
									);
									if let Ok(VirtioDriver::Network(drv)) =
										mmio_virtio::init_device(mmio, irq.try_into().unwrap())
										&& let Some(gic) = GIC.lock().as_mut()
									{
										// enable timer interrupt
										let virtio_irqid = if irqtype == 1 {
											IntId::ppi(irq)
										} else if irqtype == 0 {
											IntId::spi(irq)
										} else {
											panic!("Invalid interrupt type");
										};
										gic.set_interrupt_priority(
											virtio_irqid,
											Some(cpu_id),
											0x00,
										);
										if (irqflags & 0xf) == 4 || (irqflags & 0xf) == 8 {
											gic.set_trigger(
												virtio_irqid,
												Some(cpu_id),
												Trigger::Level,
											);
										} else if (irqflags & 0xf) == 2 || (irqflags & 0xf) == 1 {
											gic.set_trigger(
												virtio_irqid,
												Some(cpu_id),
												Trigger::Edge,
											);
										} else {
											panic!("Invalid interrupt level!");
										}
										gic.enable_interrupt(virtio_irqid, Some(cpu_id), true);

										*NETWORK_DEVICE.lock() = Some(drv);
									}
								}
								#[cfg(feature = "console")]
								virtio::Id::Console => {
									debug!(
										"Found console at {mmio:p}, irq: {irq}, type: {irqtype}, flags: {irqflags}"
									);
									if let Ok(VirtioDriver::Console(drv)) =
										mmio_virtio::init_device(mmio, irq.try_into().unwrap())
									{
										if let Some(gic) = GIC.lock().as_mut() {
											// enable timer interrupt
											let virtio_irqid = if irqtype == 1 {
												IntId::ppi(irq)
											} else if irqtype == 0 {
												IntId::spi(irq)
											} else {
												panic!("Invalid interrupt type");
											};
											gic.set_interrupt_priority(
												virtio_irqid,
												Some(cpu_id),
												0x00,
											);
											if (irqflags & 0xf) == 4 || (irqflags & 0xf) == 8 {
												gic.set_trigger(
													virtio_irqid,
													Some(cpu_id),
													Trigger::Level,
												);
											} else if (irqflags & 0xf) == 2 || (irqflags & 0xf) == 1
											{
												gic.set_trigger(
													virtio_irqid,
													Some(cpu_id),
													Trigger::Edge,
												);
											} else {
												panic!("Invalid interrupt level!");
											}
											gic.enable_interrupt(virtio_irqid, Some(cpu_id), true);

											register_driver(MmioDriver::VirtioConsole(
												hermit_sync::InterruptTicketMutex::new(*drv),
											));
										}
									}
								}
								_ => {}
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

	#[cfg(feature = "console")]
	{
		if get_console_driver().is_some() {
			info!("Switch to virtio console");
			crate::console::CONSOLE
				.lock()
				.replace_device(IoDevice::Virtio(VirtioUART::new()));
		}
	}
}
