#![allow(dead_code)]

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::fmt;

use ahash::RandomState;
use hashbrown::HashMap;
#[cfg(any(
	feature = "tcp",
	feature = "udp",
	feature = "fuse",
	feature = "vsock",
	feature = "balloon"
))]
use hermit_sync::InterruptTicketMutex;
use hermit_sync::without_interrupts;
use memory_addresses::{PhysAddr, VirtAddr};
use pci_types::capability::CapabilityIterator;
use pci_types::{
	Bar, CommandRegister, ConfigRegionAccess, DeviceId, EndpointHeader, InterruptLine,
	InterruptPin, MAX_BARS, PciAddress, PciHeader, StatusRegister, VendorId,
};

use crate::arch::pci::PciConfigRegion;
#[cfg(feature = "balloon")]
use crate::drivers::balloon::VirtioBalloonDriver;
#[cfg(feature = "fuse")]
use crate::drivers::fs::virtio_fs::VirtioFsDriver;
#[cfg(any(feature = "tcp", feature = "udp"))]
use crate::drivers::net::NetworkDriver;
#[cfg(all(target_arch = "x86_64", feature = "rtl8139"))]
use crate::drivers::net::rtl8139::{self, RTL8139Driver};
#[cfg(all(
	not(all(target_arch = "x86_64", feature = "rtl8139")),
	any(feature = "tcp", feature = "udp")
))]
use crate::drivers::net::virtio::VirtioNetDriver;
#[cfg(any(
	all(
		any(feature = "tcp", feature = "udp"),
		not(all(target_arch = "x86_64", feature = "rtl8139"))
	),
	feature = "fuse",
	feature = "vsock",
	feature = "balloon"
))]
use crate::drivers::virtio::transport::pci as pci_virtio;
#[cfg(any(
	all(
		any(feature = "tcp", feature = "udp"),
		not(all(target_arch = "x86_64", feature = "rtl8139"))
	),
	feature = "fuse",
	feature = "vsock",
	feature = "balloon"
))]
use crate::drivers::virtio::transport::pci::VirtioDriver;
#[cfg(feature = "vsock")]
use crate::drivers::vsock::VirtioVsockDriver;
#[allow(unused_imports)]
use crate::drivers::{Driver, InterruptHandlerQueue};
use crate::env;
use crate::init_cell::InitCell;

pub(crate) static PCI_DEVICES: InitCell<Vec<PciDevice<PciConfigRegion>>> =
	InitCell::new(Vec::new());
static PCI_DRIVERS: InitCell<Vec<PciDriver>> = InitCell::new(Vec::new());

#[derive(Copy, Clone, Debug)]
pub(crate) struct PciDevice<T: ConfigRegionAccess> {
	address: PciAddress,
	access: T,
}

impl<T: ConfigRegionAccess> PciDevice<T> {
	pub const fn new(address: PciAddress, access: T) -> Self {
		Self { address, access }
	}

	pub fn access(&self) -> &T {
		&self.access
	}

	pub fn header(&self) -> PciHeader {
		PciHeader::new(self.address)
	}

	/// Set flag to the command register
	pub fn set_command(&self, cmd: CommandRegister) {
		self.header()
			.update_command(&self.access, |command| command | cmd);
	}

	/// Returns the bar at bar-register `slot`.
	pub fn get_bar(&self, slot: u8) -> Option<Bar> {
		let header = self.header();
		if let Some(endpoint) = EndpointHeader::from_header(header, &self.access) {
			return endpoint.bar(slot, &self.access);
		}

		None
	}

	/// Configure the bar at register `slot`
	pub fn set_bar(&self, slot: u8, bar: Bar) {
		let value = match bar {
			Bar::Io { port } => (port | 1) as usize,
			Bar::Memory32 {
				address,
				size: _,
				prefetchable,
			} => {
				if prefetchable {
					(address | (1 << 3)) as usize
				} else {
					address as usize
				}
			}
			Bar::Memory64 {
				address,
				size: _,
				prefetchable,
			} => {
				if prefetchable {
					(address | (2 << 1) | (1 << 3)) as usize
				} else {
					(address | (2 << 1)) as usize
				}
			}
		};
		let mut header = EndpointHeader::from_header(self.header(), &self.access).unwrap();
		unsafe {
			header.write_bar(slot, &self.access, value).unwrap();
		}
	}

	/// Memory maps pci bar with specified index to identical location in virtual memory.
	/// no_cache determines if we set the `Cache Disable` flag in the page-table-entry.
	/// Returns (virtual-pointer, size) if successful, else None (if bar non-existent or IOSpace)
	pub fn memory_map_bar(&self, index: u8, no_cache: bool) -> Option<(VirtAddr, usize)> {
		let (address, size, prefetchable, width) = match self.get_bar(index) {
			Some(Bar::Io { .. }) => {
				warn!("Cannot map IOBar!");
				return None;
			}
			Some(Bar::Memory32 {
				address,
				size,
				prefetchable,
			}) => (
				u64::from(address),
				usize::try_from(size).unwrap(),
				prefetchable,
				32,
			),
			Some(Bar::Memory64 {
				address,
				size,
				prefetchable,
			}) => (address, usize::try_from(size).unwrap(), prefetchable, 64),
			_ => {
				return None;
			}
		};

		if address == 0 {
			return None;
		}

		debug!("Mapping bar {index} at {address:#x} with length {size:#x}");

		if width != 64 {
			warn!("Currently only mapping of 64 bit bars is supported!");
			return None;
		}
		if !prefetchable {
			warn!("Currently only mapping of prefetchable bars is supported!");
		}

		// Since the bios/bootloader manages the physical address space, the address got from the bar is unique and not overlapping.
		// We therefore do not need to reserve any additional memory in our kernel.
		// Map bar into RW^X virtual memory
		let physical_address = address;
		let virtual_address = if env::is_uefi() {
			VirtAddr::new(address)
		} else {
			crate::mm::map(PhysAddr::new(physical_address), size, true, true, no_cache)
		};

		Some((virtual_address, size))
	}

	pub fn get_irq(&self) -> Option<InterruptLine> {
		let header = self.header();
		if let Some(endpoint) = EndpointHeader::from_header(header, &self.access) {
			let (_pin, line) = endpoint.interrupt(&self.access);
			Some(line)
		} else {
			None
		}
	}

	pub fn set_irq(&self, pin: InterruptPin, line: InterruptLine) {
		let mut header = EndpointHeader::from_header(self.header(), &self.access).unwrap();
		header.update_interrupt(&self.access, |(_pin, _line)| (pin, line));
	}

	pub fn device_id(&self) -> DeviceId {
		let (_vendor_id, device_id) = self.header().id(&self.access);
		device_id
	}

	pub fn id(&self) -> (VendorId, DeviceId) {
		self.header().id(&self.access)
	}

	pub fn status(&self) -> StatusRegister {
		self.header().status(&self.access)
	}

	pub fn capabilities(&self) -> Option<CapabilityIterator<&T>> {
		EndpointHeader::from_header(self.header(), &self.access)
			.map(|header| header.capabilities(&self.access))
	}
}

impl<T: ConfigRegionAccess> fmt::Display for PciDevice<T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let header = self.header();
		let header_type = header.header_type(&self.access);
		let (vendor_id, device_id) = header.id(&self.access);
		let (_dev_rev, class_id, subclass_id, _interface) = header.revision_and_class(&self.access);

		if let Some(endpoint) = EndpointHeader::from_header(header, &self.access) {
			#[cfg(feature = "pci-ids")]
			let (class_name, vendor_name, device_name) = {
				use pci_ids::{Class, Device, FromId, Subclass};

				let class_name = Class::from_id(class_id).map_or("Unknown Class", |class| {
					class
						.subclasses()
						.find(|s| s.id() == subclass_id)
						.map(Subclass::name)
						.unwrap_or_else(|| class.name())
				});

				let (vendor_name, device_name) = Device::from_vid_pid(vendor_id, device_id)
					.map(|device| (device.vendor().name(), device.name()))
					.unwrap_or(("Unknown Vendor", "Unknown Device"));

				(class_name, vendor_name, device_name)
			};

			#[cfg(not(feature = "pci-ids"))]
			let (class_name, vendor_name, device_name) =
				("Unknown Class", "Unknown Vendor", "Unknown Device");

			// Output detailed readable information about this device.
			write!(
				f,
				"{:02X}:{:02X} {} [{:02X}{:02X}]: {} {} [{:04X}:{:04X}]",
				self.address.bus(),
				self.address.device(),
				class_name,
				class_id,
				subclass_id,
				vendor_name,
				device_name,
				vendor_id,
				device_id
			)?;

			// If the devices uses an IRQ, output this one as well.
			let (_, irq) = endpoint.interrupt(&self.access);
			if irq != 0 && irq != u8::MAX {
				write!(f, ", IRQ {irq}")?;
			}

			let mut slot: u8 = 0;
			while usize::from(slot) < MAX_BARS {
				if let Some(pci_bar) = endpoint.bar(slot, &self.access) {
					match pci_bar {
						Bar::Memory64 {
							address,
							size,
							prefetchable,
						} => {
							write!(
								f,
								", BAR{slot} Memory64 {{ address: {address:#X}, size: {size:#X}, prefetchable: {prefetchable} }}"
							)?;
							slot += 1;
						}
						Bar::Memory32 {
							address,
							size,
							prefetchable,
						} => {
							write!(
								f,
								", BAR{slot} Memory32 {{ address: {address:#X}, size: {size:#X}, prefetchable: {prefetchable} }}"
							)?;
						}
						Bar::Io { port } => {
							write!(f, ", BAR{slot} IO {{ port: {port:#X} }}")?;
						}
					}
				}
				slot += 1;
			}
		} else {
			// Output detailed readable information about this device.
			write!(
				f,
				"{:02X}:{:02X} {:?} [{:04X}:{:04X}]",
				self.address.bus(),
				self.address.device(),
				header_type,
				vendor_id,
				device_id
			)?;
		}

		Ok(())
	}
}

pub(crate) fn print_information() {
	infoheader!(" PCI BUS INFORMATION ");

	for adapter in PCI_DEVICES.finalize().iter() {
		info!("{adapter}");
	}

	infofooter!();
}

#[allow(clippy::large_enum_variant)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum PciDriver {
	#[cfg(feature = "fuse")]
	VirtioFs(InterruptTicketMutex<VirtioFsDriver>),
	#[cfg(feature = "vsock")]
	VirtioVsock(InterruptTicketMutex<VirtioVsockDriver>),
	#[cfg(all(
		not(all(target_arch = "x86_64", feature = "rtl8139")),
		any(feature = "tcp", feature = "udp")
	))]
	VirtioNet(InterruptTicketMutex<VirtioNetDriver>),
	#[cfg(all(
		target_arch = "x86_64",
		feature = "rtl8139",
		any(feature = "tcp", feature = "udp")
	))]
	RTL8139Net(InterruptTicketMutex<RTL8139Driver>),
	#[cfg(feature = "balloon")]
	VirtioBalloon(InterruptTicketMutex<VirtioBalloonDriver>),
}

impl PciDriver {
	#[cfg(all(
		not(all(target_arch = "x86_64", feature = "rtl8139")),
		any(feature = "tcp", feature = "udp")
	))]
	fn get_network_driver(&self) -> Option<&InterruptTicketMutex<VirtioNetDriver>> {
		#[allow(unreachable_patterns)]
		match self {
			Self::VirtioNet(drv) => Some(drv),
			_ => None,
		}
	}

	#[cfg(all(
		target_arch = "x86_64",
		feature = "rtl8139",
		any(feature = "tcp", feature = "udp")
	))]
	fn get_network_driver(&self) -> Option<&InterruptTicketMutex<RTL8139Driver>> {
		#[allow(unreachable_patterns)]
		match self {
			Self::RTL8139Net(drv) => Some(drv),
			_ => None,
		}
	}

	#[cfg(feature = "vsock")]
	fn get_vsock_driver(&self) -> Option<&InterruptTicketMutex<VirtioVsockDriver>> {
		#[allow(unreachable_patterns)]
		match self {
			Self::VirtioVsock(drv) => Some(drv),
			_ => None,
		}
	}

	#[cfg(feature = "fuse")]
	fn get_filesystem_driver(&self) -> Option<&InterruptTicketMutex<VirtioFsDriver>> {
		match self {
			Self::VirtioFs(drv) => Some(drv),
			#[allow(unreachable_patterns)]
			_ => None,
		}
	}

	#[cfg(feature = "balloon")]
	fn get_balloon_driver(&self) -> Option<&InterruptTicketMutex<VirtioBalloonDriver>> {
		#[allow(unreachable_patterns)]
		match self {
			Self::VirtioBalloon(drv) => Some(drv),
			_ => None,
		}
	}

	fn get_interrupt_handler(&self) -> (InterruptLine, fn()) {
		#[allow(unreachable_patterns)]
		match self {
			#[cfg(feature = "vsock")]
			Self::VirtioVsock(drv) => {
				fn vsock_handler() {
					if let Some(driver) = get_vsock_driver() {
						driver.lock().handle_interrupt();
					}
				}

				let irq_number = drv.lock().get_interrupt_number();

				(irq_number, vsock_handler)
			}
			#[cfg(all(
				target_arch = "x86_64",
				feature = "rtl8139",
				any(feature = "tcp", feature = "udp")
			))]
			Self::RTL8139Net(drv) => {
				fn rtl8139_handler() {
					if let Some(driver) = get_network_driver() {
						driver.lock().handle_interrupt();
					}
				}

				let irq_number = drv.lock().get_interrupt_number();

				(irq_number, rtl8139_handler)
			}
			#[cfg(all(
				not(all(target_arch = "x86_64", feature = "rtl8139")),
				any(feature = "tcp", feature = "udp")
			))]
			Self::VirtioNet(drv) => {
				fn network_handler() {
					if let Some(driver) = get_network_driver() {
						driver.lock().handle_interrupt();
					}
				}

				let irq_number = drv.lock().get_interrupt_number();

				(irq_number, network_handler)
			}
			#[cfg(feature = "fuse")]
			Self::VirtioFs(drv) => {
				fn fuse_handler() {}

				let irq_number = drv.lock().get_interrupt_number();

				(irq_number, fuse_handler)
			}
			#[cfg(feature = "balloon")]
			Self::VirtioBalloon(drv) => {
				fn balloon_handler() {
					if let Some(driver) = get_balloon_driver() {
						driver.lock().handle_interrupt();
					}
				}

				let irq_number = drv.lock().get_interrupt_number();

				(irq_number, balloon_handler)
			}
			_ => todo!(),
		}
	}
}

pub(crate) fn register_driver(drv: PciDriver) {
	PCI_DRIVERS.with(|pci_drivers| pci_drivers.unwrap().push(drv));
}

pub(crate) fn get_interrupt_handlers() -> HashMap<InterruptLine, InterruptHandlerQueue, RandomState>
{
	let mut handlers: HashMap<InterruptLine, InterruptHandlerQueue, RandomState> =
		HashMap::with_hasher(RandomState::with_seeds(0, 0, 0, 0));

	for drv in PCI_DRIVERS.finalize().iter() {
		let (irq_number, handler) = drv.get_interrupt_handler();

		if let Some(map) = handlers.get_mut(&irq_number) {
			map.push_back(handler);
		} else {
			let mut map: InterruptHandlerQueue = VecDeque::new();
			map.push_back(handler);
			handlers.insert(irq_number, map);
		}
	}

	handlers
}

#[cfg(all(
	not(all(target_arch = "x86_64", feature = "rtl8139")),
	any(feature = "tcp", feature = "udp")
))]
pub(crate) fn get_network_driver() -> Option<&'static InterruptTicketMutex<VirtioNetDriver>> {
	PCI_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_network_driver())
}

#[cfg(all(
	target_arch = "x86_64",
	feature = "rtl8139",
	any(feature = "tcp", feature = "udp")
))]
pub(crate) fn get_network_driver() -> Option<&'static InterruptTicketMutex<RTL8139Driver>> {
	PCI_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_network_driver())
}

#[cfg(feature = "vsock")]
pub(crate) fn get_vsock_driver() -> Option<&'static InterruptTicketMutex<VirtioVsockDriver>> {
	PCI_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_vsock_driver())
}

#[cfg(feature = "fuse")]
pub(crate) fn get_filesystem_driver() -> Option<&'static InterruptTicketMutex<VirtioFsDriver>> {
	PCI_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_filesystem_driver())
}

#[cfg(feature = "balloon")]
pub(crate) fn get_balloon_driver() -> Option<&'static InterruptTicketMutex<VirtioBalloonDriver>> {
	PCI_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_balloon_driver())
}

pub(crate) fn init() {
	// virtio: 4.1.2 PCI Device Discovery
	without_interrupts(|| {
		for adapter in PCI_DEVICES.finalize().iter().filter(|x| {
			let (vendor_id, device_id) = x.id();
			vendor_id == 0x1af4 && (0x1000..=0x107f).contains(&device_id)
		}) {
			info!(
				"Found virtio device with device id {:#x}",
				adapter.device_id()
			);

			#[cfg(any(
				all(
					any(feature = "tcp", feature = "udp"),
					not(all(target_arch = "x86_64", feature = "rtl8139"))
				),
				feature = "fuse",
				feature = "vsock",
				feature = "balloon"
			))]
			match pci_virtio::init_device(adapter) {
				#[cfg(all(
					not(all(target_arch = "x86_64", feature = "rtl8139")),
					any(feature = "tcp", feature = "udp")
				))]
				Ok(VirtioDriver::Network(drv)) => {
					register_driver(PciDriver::VirtioNet(InterruptTicketMutex::new(drv)));
				}
				#[cfg(feature = "vsock")]
				Ok(VirtioDriver::Vsock(drv)) => {
					register_driver(PciDriver::VirtioVsock(InterruptTicketMutex::new(*drv)));
				}
				#[cfg(feature = "fuse")]
				Ok(VirtioDriver::FileSystem(drv)) => {
					register_driver(PciDriver::VirtioFs(InterruptTicketMutex::new(drv)));
				}
				#[cfg(feature = "balloon")]
				Ok(VirtioDriver::Balloon(drv)) => {
					register_driver(PciDriver::VirtioBalloon(InterruptTicketMutex::new(drv)));
				}
				_ => {}
			}
		}

		// Searching for Realtek RTL8139, which is supported by Qemu
		#[cfg(all(target_arch = "x86_64", feature = "rtl8139"))]
		for adapter in PCI_DEVICES.finalize().iter().filter(|x| {
			let (vendor_id, device_id) = x.id();
			vendor_id == 0x10ec && (0x8138..=0x8139).contains(&device_id)
		}) {
			info!(
				"Found Realtek network device with device id {:#x}",
				adapter.device_id()
			);

			if let Ok(drv) = rtl8139::init_device(adapter) {
				register_driver(PciDriver::RTL8139Net(InterruptTicketMutex::new(drv)));
			}
		}
	});
}

/// A module containing PCI specific errors
///
/// Errors include...
pub(crate) mod error {
	/// An enum of PciErrors
	/// typically carrying the device's id as an u16.
	#[derive(Debug)]
	pub enum PciError {
		General(u16),
		NoBar(u16),
		NoCapPtr(u16),
		NoVirtioCaps(u16),
	}
}
