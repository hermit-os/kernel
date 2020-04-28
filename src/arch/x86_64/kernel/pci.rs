// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use alloc::rc::Rc;
use alloc::vec::Vec;
use arch::x86_64::kernel::pci_ids::{CLASSES, VENDORS};
use arch::x86_64::kernel::virtio;
use arch::x86_64::kernel::virtio_fs::VirtiofsDriver;
use core::cell::RefCell;
use core::{fmt, u32, u8};
use synch::spinlock::Spinlock;
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
pub const PCI_BAR0_REGISTER: u32 = 0x10;
pub const PCI_CAPABILITY_LIST_REGISTER: u32 = 0x34;
pub const PCI_INTERRUPT_REGISTER: u32 = 0x3C;

pub const PCI_STATUS_CAPABILITIES_LIST: u32 = 1 << 4;

pub const PCI_BASE_ADDRESS_IO_SPACE: u32 = 1 << 0;
pub const PCI_BASE_ADDRESS_64BIT: u32 = 1 << 2;
pub const PCI_BASE_ADDRESS_MASK: u32 = 0xFFFF_FFF0;

pub const PCI_CAP_ID_VNDR: u32 = 0x09;

static PCI_ADAPTERS: Spinlock<Vec<PciAdapter>> = Spinlock::new(Vec::new());
static PCI_DRIVERS: Spinlock<Vec<PciDriver>> = Spinlock::new(Vec::new());

#[derive(Clone, Copy)]
pub struct PciAdapter {
	pub bus: u8,
	pub device: u8,
	pub vendor_id: u16,
	pub device_id: u16,
	pub class_id: u8,
	pub subclass_id: u8,
	pub programming_interface_id: u8,
	pub base_addresses: [u32; 6],
	pub base_sizes: [u32; 6],
	pub irq: u8,
}

pub enum PciDriver<'a> {
	VirtioFs(Rc<RefCell<VirtiofsDriver<'a>>>),
}

pub fn register_driver(drv: PciDriver<'static>) {
	let mut drivers = PCI_DRIVERS.lock();
	drivers.push(drv);
}

impl PciAdapter {
	fn new(bus: u8, device: u8, vendor_id: u16, device_id: u16) -> Self {
		// TODO: check Header_Type for 0x00 (general purpose device), since irq is not defined otherwise!
		// TODO: warn if header_type specifies multifunciton, since we dont scan additional functions
		let class_ids = read_config(bus, device, PCI_CLASS_REGISTER);

		let mut base_addresses: [u32; 6] = [0; 6];
		let mut base_sizes: [u32; 6] = [0; 6];
		// TODO: this only works for I/O Space BARs! Verify that bit 0 is 1!
		for i in 0..6 {
			let register = PCI_BAR0_REGISTER + ((i as u32) << 2);
			let barword = read_config(bus, device, register);
			if barword & 1 == 0 {
				debug!("Bar {} @{:x}:{:x} is memory mapped, but treated as IO mapped! this will cause errors later..", i, device_id, vendor_id);
			}
			base_addresses[i] = barword & 0xFFFF_FFFC;

			if base_addresses[i] > 0 {
				write_config(bus, device, register, u32::MAX);
				base_sizes[i] = !(read_config(bus, device, register) & PCI_BASE_ADDRESS_MASK) + 1;
				write_config(bus, device, register, base_addresses[i]);
			}
		}

		let interrupt_info = read_config(bus, device, PCI_INTERRUPT_REGISTER);

		Self {
			bus: bus,
			device: device,
			vendor_id: vendor_id,
			device_id: device_id,
			class_id: (class_ids >> 24) as u8,
			subclass_id: (class_ids >> 16) as u8,
			programming_interface_id: (class_ids >> 8) as u8,
			base_addresses: base_addresses,
			base_sizes: base_sizes,
			irq: interrupt_info as u8,
		}
	}

	pub fn make_bus_master(&self) {
		let mut command = read_config(self.bus, self.device, PCI_COMMAND_REGISTER);
		command |= PCI_COMMAND_BUSMASTER;
		write_config(self.bus, self.device, PCI_COMMAND_REGISTER, command);
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

		write!(f, ", iobase ")?;
		for i in 0..self.base_addresses.len() {
			if self.base_addresses[i] > 0 {
				write!(
					f,
					"0x{:x} (size 0x{:x}) ",
					self.base_addresses[i], self.base_sizes[i]
				)?;
			}
		}

		Ok(())
	}
}

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
			return Some(*adapter);
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
				adapters.push(adapter);
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
			virtio::init_virtio_device(*adapter);
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
