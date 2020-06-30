// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//               2020 Thomas Lambertz, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use alloc::rc::Rc;
use alloc::vec::Vec;
use arch::x86_64::kernel::pci_ids::{CLASSES, VENDORS};
use drivers::virtio::depr::virtio_fs::VirtioFsDriver;
use drivers::virtio::depr::virtio_net::VirtioNetDriver;
use drivers::net::virtio_net::VirtioNetDriver as VnetDrv;
use drivers::virtio::depr::virtio;
//use drivers::virtio::transport::pci as pci_virtio;
use core::cell::RefCell;
use core::convert::TryInto;
use core::{fmt, u32, u8};
use synch::spinlock::SpinlockIrqSave;
use x86::io::*;

// TODO: should these be pub? currently needed since used in virtio.rs maybe use getter methods to be more flexible.
pub const PCI_MAX_BUS_NUMBER: u8 = 32;
pub const PCI_MAX_DEVICE_NUMBER: u8 = 32;

pub const PCI_CONFIG_ADDRESS_PORT: u16 = 0xCF8;
pub const PCI_CONFIG_ADDRESS_ENABLE: u32 = 1 << 31;

pub const PCI_CONFIG_DATA_PORT: u16 = 0xCFC;
pub const PCI_COMMAND_BUSMASTER: u32 = 1 << 2;

pub const PCI_ID_REGISTER: u32 = 0x00;
pub const PCI_COMMAND_REGISTER: u32 = 0x04;
pub const PCI_CLASS_REGISTER: u32 = 0x08;
pub const PCI_HEADER_REGISTER: u32 = 0x0C;
pub const PCI_BAR0_REGISTER: u32 = 0x10;
pub const PCI_CAPABILITY_LIST_REGISTER: u32 = 0x34;
pub const PCI_INTERRUPT_REGISTER: u32 = 0x3C;

pub const PCI_STATUS_CAPABILITIES_LIST: u32 = 1 << 4;

pub const PCI_BASE_ADDRESS_IO_SPACE: u32 = 1 << 0;
pub const PCI_MEM_BASE_ADDRESS_64BIT: u32 = 1 << 2;
pub const PCI_MEM_PREFETCHABLE: u32 = 1 << 3;
pub const PCI_MEM_BASE_ADDRESS_MASK: u32 = 0xFFFF_FFF0;
pub const PCI_IO_BASE_ADDRESS_MASK: u32 = 0xFFFF_FFFC;

pub const PCI_HEADER_TYPE_MASK: u32 = 0x007F_0000;
pub const PCI_MULTIFUNCTION_MASK: u32 = 0x0080_0000;

pub const PCI_CAP_ID_VNDR: u32 = 0x09;

static PCI_ADAPTERS: SpinlockIrqSave<Vec<PciAdapter>> = SpinlockIrqSave::new(Vec::new());
static PCI_DRIVERS: SpinlockIrqSave<Vec<PciDriver>> = SpinlockIrqSave::new(Vec::new());

/// Classes of PCI nodes.
#[allow(dead_code)]
#[derive(Copy, Clone, Debug, FromPrimitive, ToPrimitive, PartialEq)]
pub enum PciClassCode {
	TooOld = 0x00,
	MassStorage = 0x01,
	NetworkController = 0x02,
	DisplayController = 0x03,
	MultimediaController = 0x04,
	MemoryController = 0x05,
	BridgeDevice = 0x06,
	SimpleCommunicationController = 0x07,
	BaseSystemPeripheral = 0x08,
	InputDevice = 0x09,
	DockingStation = 0x0A,
	Processor = 0x0B,
	SerialBusController = 0x0C,
	WirelessController = 0x0D,
	IntelligentIoController = 0x0E,
	EncryptionController = 0x0F,
	DataAcquisitionSignalProcessing = 0x11,
	Other = 0xFF,
}

/// Network Controller Sub Classes
#[allow(dead_code)]
#[derive(Copy, Clone, Debug, FromPrimitive, ToPrimitive, PartialEq)]
pub enum PciNetworkControllerSubclass {
	EthernetController = 0x00,
	TokenRingController = 0x01,
	FDDIController = 0x02,
	ATMController = 0x03,
	ISDNController = 0x04,
	WorldFipController = 0x05,
	PICMGController = 0x06,
	InfinibandController = 0x07,
	FabricController = 0x08,
	NetworkController = 0x80,
}

#[derive(Clone, Debug)]
pub struct PciAdapter {
	pub bus: u8,
	pub device: u8,
	pub vendor_id: u16,
	pub device_id: u16,
	pub class_id: u8,
	pub subclass_id: u8,
	pub programming_interface_id: u8,
	pub base_addresses: Vec<PciBar>,
	pub irq: u8,
}
#[derive(Clone, Copy, Debug)]
pub enum PciBar {
	IO(IOBar),
	Memory(MemoryBar),
}
#[derive(Clone, Copy, Debug)]
pub struct IOBar {
	pub index: u8,
	pub addr: u32,
	pub size: usize,
}
#[derive(Clone, Copy, Debug)]
pub struct MemoryBar {
	pub index: u8,
	pub addr: usize,
	pub size: usize,
	pub width: u8, // 32 or 64 bit
	pub prefetchable: bool,
}

pub enum PciDriver<'a> {
	VirtioFs(Rc<RefCell<VirtioFsDriver<'a>>>),
	VirtioNet(Rc<RefCell<VirtioNetDriver<'a>>>),
	VirtioNetNew(VnetDrv),
}

pub fn register_driver(drv: PciDriver<'static>) {
	let mut drivers = PCI_DRIVERS.lock();
	drivers.push(drv);
}

pub fn get_network_driver() -> Option<Rc<RefCell<VirtioNetDriver<'static>>>> {
	let drivers = PCI_DRIVERS.lock();
	for i in drivers.iter() {
		match &*i {
			PciDriver::VirtioNet(nic_driver) => {
				return Some(nic_driver.clone());
			}
			_ => {}
		}
	}

	None
}

/// Reads all bar registers of specified device and returns vector of PciBar's containing addresses and sizes.
fn parse_bars(bus: u8, device: u8, vendor_id: u16, device_id: u16) -> Vec<PciBar> {
	let mut bar_idxs = 0..6;
	let mut bars = Vec::new();
	while let Some(i) = bar_idxs.next() {
		let register = PCI_BAR0_REGISTER + ((i as u32) << 2);
		let barword = read_config(bus, device, register);
		debug!(
			"Found bar{} @{:x}:{:x} as 0x{:x}",
			i, vendor_id, device_id, barword
		);

		// We assume BIOS or something similar has initialized the device already and set appropriate values into the bar registers

		// If barword is all 0, the bar is disabled
		if barword == 0 {
			continue;
		}

		// Determine if bar is IO-mapped or memory-mapped
		if barword & PCI_BASE_ADDRESS_IO_SPACE != 0 {
			// IO Mapped BAR
			debug!("Bar {} @{:x}:{:x} IO mapped!", i, vendor_id, device_id);

			let base_addr = barword & PCI_IO_BASE_ADDRESS_MASK;

			// determine size by writing 0xFFFFFFFF
			write_config(bus, device, register, u32::MAX);
			let sizebits = read_config(bus, device, register);
			// Restore original value of register
			write_config(bus, device, register, barword);
			let size = (!(sizebits & PCI_IO_BASE_ADDRESS_MASK) + 1) as usize;

			bars.push(PciBar::IO(IOBar {
				index: i as u8,
				addr: base_addr,
				size,
			}));
		} else {
			// Memory Mapped BAR
			let prefetchable = barword & PCI_MEM_PREFETCHABLE != 0;

			if barword & PCI_MEM_BASE_ADDRESS_64BIT != 0 {
				// 64-bit, load additional bar-word
				let register_high = PCI_BAR0_REGISTER + (bar_idxs.next().unwrap() << 2);
				let barword_high = read_config(bus, device, register_high);

				let base_addr = ((barword_high as usize) << 32) + (barword & 0xFFFF_FFF0) as usize;
				debug!(
					"64-bit memory bar, merged next barword. Addr: 0x{:x}",
					base_addr
				);

				// determine size by writing 0xFFFFFFFF
				write_config(bus, device, register, u32::MAX);
				let sizebits = read_config(bus, device, register);

				// Also read/write to register_high if needed
				let size = if sizebits == 0 {
					write_config(bus, device, register_high, u32::MAX);
					let sizebits = read_config(bus, device, register_high);
					// Restore original value of register_high
					write_config(bus, device, register_high, barword);

					((!sizebits + 1) as usize) << 32
				} else {
					(!(sizebits & PCI_MEM_BASE_ADDRESS_MASK) + 1) as usize
				};

				// Restore original value
				write_config(bus, device, register, barword);

				bars.push(PciBar::Memory(MemoryBar {
					index: i as u8,
					addr: base_addr,
					size,
					width: 64,
					prefetchable,
				}));
			} else {
				// 32-bit
				let base_addr = (barword & 0xFFFF_FFF0) as usize;

				// determine size by writing 0xFFFFFFFF
				write_config(bus, device, register, u32::MAX);
				let size = !(read_config(bus, device, register) & PCI_MEM_BASE_ADDRESS_MASK) + 1;

				// Restore original value
				write_config(bus, device, register, barword);

				bars.push(PciBar::Memory(MemoryBar {
					index: i as u8,
					addr: base_addr,
					size: size.try_into().unwrap(),
					width: 32,
					prefetchable,
				}));
			}
		}
	}

	return bars;
}

impl PciAdapter {
	fn new(bus: u8, device: u8, vendor_id: u16, device_id: u16) -> Option<Self> {
		let header = read_config(bus, device, PCI_HEADER_REGISTER);
		if header & PCI_HEADER_TYPE_MASK != 0 {
			error!(
				"PCI Device @{:x}:{:x} does not have header type 0!",
				vendor_id, device_id
			);
			return None;
		}
		if header & PCI_MULTIFUNCTION_MASK != 0 {
			warn!(
				"PCI Device @{:x}:{:x} has multiple functions! Currently only one is handled.",
				vendor_id, device_id
			);
		}

		let class_ids = read_config(bus, device, PCI_CLASS_REGISTER);
		let bars = parse_bars(bus, device, vendor_id, device_id);
		let interrupt_info = read_config(bus, device, PCI_INTERRUPT_REGISTER);

		Some(Self {
			bus: bus,
			device: device,
			vendor_id: vendor_id,
			device_id: device_id,
			class_id: (class_ids >> 24) as u8,
			subclass_id: (class_ids >> 16) as u8,
			programming_interface_id: (class_ids >> 8) as u8,
			base_addresses: bars,
			irq: interrupt_info as u8,
		})
	}

	pub fn make_bus_master(&self) {
		let mut command = read_config(self.bus, self.device, PCI_COMMAND_REGISTER);
		command |= PCI_COMMAND_BUSMASTER;
		write_config(self.bus, self.device, PCI_COMMAND_REGISTER, command);
	}

	/// Returns the bar at bar-register baridx.
	pub fn get_bar(&self, baridx: u8) -> Option<PciBar> {
		for bar in &self.base_addresses {
			match bar {
				PciBar::IO(bar) => {
					if bar.index == baridx {
						return Some(PciBar::IO(*bar));
					}
				}
				PciBar::Memory(bar) => {
					if bar.index == baridx {
						return Some(PciBar::Memory(*bar));
					}
				}
			}
		}
		return None;
	}

	/// Memory maps pci bar with specified index to identical location in virtual memory.
	/// no_cache determines if we set the `Cache Disable` flag in the page-table-entry.
	/// Returns (virtual-pointer, size) if successful, else None (if bar non-existent or IOSpace)
	pub fn memory_map_bar(&self, index: u8, no_cache: bool) -> Option<(usize, usize)> {
		let bar = match self.get_bar(index) {
			Some(PciBar::IO(_)) => {
				warn!("Cannot map IOBar!");
				return None;
			}
			Some(PciBar::Memory(bar)) => bar,
			None => {
				warn!("Memory bar not found!");
				return None;
			}
		};

		debug!(
			"Mapping bar {} at 0x{:x} with length 0x{:x}",
			index, bar.addr, bar.size
		);

		if bar.width != 64 {
			warn!("Currently only mapping of 64 bit bars is supported!");
			return None;
		}
		if !bar.prefetchable {
			warn!("Currently only mapping of prefetchable bars is supported!")
		}

		// Since the bios/bootloader manages the physical address space, the address got from the bar is unique and not overlapping.
		// We therefore do not need to reserve any additional memory in our kernel.
		// Map bar into RW^X virtual memory
		let physical_address = bar.addr;
		let virtual_address = ::mm::map(physical_address, bar.size, true, false, no_cache);

		Some((virtual_address, bar.size))
	}
}

impl fmt::Display for PciBar {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let (typ, addr, size) = match self {
			PciBar::IO(bar) => ("IOBar", bar.addr as usize, bar.size as usize),
			PciBar::Memory(bar) => ("MemoryBar", bar.addr, bar.size),
		};
		write!(f, "{}: 0x{:x} (size 0x{:x})", typ, addr, size)?;

		Ok(())
	}
}

impl fmt::Display for PciAdapter {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		// Look for the best matching class name in the PCI Database.
		let mut class_name = "Unknown Class";
		for ref c in CLASSES {
			if c.id == self.class_id {
				class_name = c.name;
				for ref sc in c.subclasses {
					if sc.id == self.subclass_id {
						class_name = sc.name;
						break;
					}
				}

				break;
			}
		}

		// Look for the vendor and device name in the PCI Database.
		let mut vendor_name = "Unknown Vendor";
		let mut device_name = "Unknown Device";
		for ref v in VENDORS {
			if v.id == self.vendor_id {
				vendor_name = v.name;
				for ref d in v.devices {
					if d.id == self.device_id {
						device_name = d.name;
						break;
					}
				}

				break;
			}
		}

		// Output detailed readable information about this device.
		write!(
			f,
			"{:02X}:{:02X} {} [{:02X}{:02X}]: {} {} [{:04X}:{:04X}]",
			self.bus,
			self.device,
			class_name,
			self.class_id,
			self.subclass_id,
			vendor_name,
			device_name,
			self.vendor_id,
			self.device_id
		)?;

		// If the devices uses an IRQ, output this one as well.
		if self.irq != 0 && self.irq != u8::MAX {
			write!(f, ", IRQ {}", self.irq)?;
		}

		for bar in &self.base_addresses {
			write!(f, ", {}", bar)?;
		}

		Ok(())
	}
}

/// Returns the value (indicated by bus, device and register) of the pci 
/// configuration space. 
pub fn read_config(bus: u8, device: u8, register: u32) -> u32 {
	let address =
		PCI_CONFIG_ADDRESS_ENABLE | u32::from(bus) << 16 | u32::from(device) << 11 | register;
	unsafe {
		outl(PCI_CONFIG_ADDRESS_PORT, address);
		inl(PCI_CONFIG_DATA_PORT)
	}
}

pub fn write_config(bus: u8, device: u8, register: u32, data: u32) {
	let address =
		PCI_CONFIG_ADDRESS_ENABLE | u32::from(bus) << 16 | u32::from(device) << 11 | register;
	unsafe {
		outl(PCI_CONFIG_ADDRESS_PORT, address);
		outl(PCI_CONFIG_DATA_PORT, data);
	}
}

pub fn get_adapter(vendor_id: u16, device_id: u16) -> Option<PciAdapter> {
	let adapters = PCI_ADAPTERS.lock();
	for adapter in adapters.iter() {
		if adapter.vendor_id == vendor_id && adapter.device_id == device_id {
			return Some(adapter.clone());
		}
	}

	None
}

pub fn init() {
	debug!("Scanning PCI Busses 0 to {}", PCI_MAX_BUS_NUMBER - 1);
	let mut adapters = PCI_ADAPTERS.lock();

	// HermitCore only uses PCI for network devices.
	// Therefore, multifunction devices as well as additional bridges are not scanned.
	// We also limit scanning to the first 32 buses.
	for bus in 0..PCI_MAX_BUS_NUMBER {
		for device in 0..PCI_MAX_DEVICE_NUMBER {
			let device_vendor_id = read_config(bus, device, PCI_ID_REGISTER);
			if device_vendor_id != u32::MAX {
				let device_id = (device_vendor_id >> 16) as u16;
				let vendor_id = device_vendor_id as u16;
				let adapter = PciAdapter::new(bus, device, vendor_id, device_id);
				if let Some(adapter) = adapter {
					adapters.push(adapter);
				}
			}
		}
	}
}

pub fn init_drivers() {
	let adapters = PCI_ADAPTERS.lock();
	// virtio: 4.1.2 PCI Device Discovery
	for adapter in adapters.iter() {
		if adapter.vendor_id == 0x1AF4 && adapter.device_id >= 0x1000 && adapter.device_id <= 0x107F
		{
			info!(
				"Found virtio device with device id 0x{:x}",
				adapter.device_id
			);

			/* New initalization function
			 * 
			 *  match pci_virtio::init_device(adapter) {
             *     Ok(PciDriver) => {
             *	       pci::register_driver(PciDriver);
			 *     },
			 *     Err() => {
			 *	       // Handle Error, probably just warn!(...)
			 *     }
			 * }
			 *  
			 */
			virtio::init_virtio_device(adapter);
		}
	}
}

pub fn print_information() {
	infoheader!(" PCI BUS INFORMATION ");

	let adapters = PCI_ADAPTERS.lock();
	for adapter in adapters.iter() {
		info!("{}", adapter);
	}

	infofooter!();
}

/// A module containg PCI specifc errors
///
/// Errors include...
pub mod error {
	/// An enum of PciErrors
	/// typically carrying the device's id as an u16.
	#[derive(Debug)]
    pub enum PciError {
		General(u16),
		NoBar(u16),
		NoCapPtr(u16),
		BadCapPtr(u16),
		NoBarForCap(u16),
		NoVirtioCaps(u16)	
    }
}