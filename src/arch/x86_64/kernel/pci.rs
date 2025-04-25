use pci_types::{ConfigRegionAccess, PciAddress, PciHeader};
use x86_64::instructions::port::Port;

use crate::drivers::pci::{PCI_DEVICES, PciDevice};

const PCI_MAX_BUS_NUMBER: u8 = 32;
const PCI_MAX_DEVICE_NUMBER: u8 = 32;

const PCI_CONFIG_ADDRESS_ENABLE: u32 = 1 << 31;

const CONFIG_ADDRESS: Port<u32> = Port::new(0xcf8);
const CONFIG_DATA: Port<u32> = Port::new(0xcfc);

#[derive(Debug, Copy, Clone)]
struct PciConfigRegion;

impl PciConfigRegion {
	pub const fn new() -> Self {
		Self {}
	}
}

#[derive(Debug, Copy, Clone)]
pub enum PciConfigAccess {
	PciConfigRegion(PciConfigRegion),
	#[cfg(feature = "acpi")]
	PcieConfigRegion(pcie::McfgTableEntry),
}

impl ConfigRegionAccess for PciConfigAccess {
	unsafe fn read(&self, address: PciAddress, offset: u16) -> u32 {
		match self {
			PciConfigAccess::PciConfigRegion(entry) => entry.read(address, offset),
			PciConfigAccess::PcieConfigRegion(entry) => entry.read(address, offset),
		}
	}

	unsafe fn write(&self, address: PciAddress, offset: u16, value: u32) {
		match self {
			PciConfigAccess::PciConfigRegion(entry) => entry.write(address, offset, value),
			PciConfigAccess::PcieConfigRegion(entry) => entry.write(address, offset, value),
		}
	}
}

impl ConfigRegionAccess for PciConfigRegion {
	#[inline]
	unsafe fn read(&self, pci_addr: PciAddress, register: u16) -> u32 {
		let mut config_address = CONFIG_ADDRESS;
		let mut config_data = CONFIG_DATA;

		let address = PCI_CONFIG_ADDRESS_ENABLE
			| (u32::from(pci_addr.bus()) << 16)
			| (u32::from(pci_addr.device()) << 11)
			| (u32::from(pci_addr.function()) << 8)
			| u32::from(register);

		unsafe {
			config_address.write(address);
			config_data.read()
		}
	}

	#[inline]
	unsafe fn write(&self, pci_addr: PciAddress, register: u16, value: u32) {
		let mut config_address = CONFIG_ADDRESS;
		let mut config_data = CONFIG_DATA;

		let address = PCI_CONFIG_ADDRESS_ENABLE
			| (u32::from(pci_addr.bus()) << 16)
			| (u32::from(pci_addr.device()) << 11)
			| (u32::from(pci_addr.function()) << 8)
			| u32::from(register);

		unsafe {
			config_address.write(address);
			config_data.write(value);
		}
	}
}

pub(crate) fn init() {
	debug!("Scanning PCI Busses 0 to {}", PCI_MAX_BUS_NUMBER - 1);

	#[cfg(feature = "acpi")]
	if pcie::init_pcie() { return; }

	enumerate_devices(0, PCI_MAX_BUS_NUMBER, PciConfigAccess::PciConfigRegion(PciConfigRegion::new()))
}

fn enumerate_devices(bus_start: u8, bus_end: u8, access: PciConfigAccess) {
	// Hermit only uses PCI for network devices.
	// Therefore, multifunction devices as well as additional bridges are not scanned.
	// We also limit scanning to the first 32 buses.
	for bus in bus_start..bus_end {
		for device in 0..PCI_MAX_DEVICE_NUMBER {
			let pci_address = PciAddress::new(0, bus, device, 0);
			let header = PciHeader::new(pci_address);

			let (device_id, vendor_id) = header.id(access);
			if device_id != u16::MAX && vendor_id != u16::MAX {
				let device = PciDevice::new(pci_address, access);
				PCI_DEVICES.with(|pci_devices| pci_devices.unwrap().push(device));
			}
		}
	}
}

#[cfg(feature = "acpi")]
mod pcie {
	use core::ptr;
	use pci_types::{ConfigRegionAccess, PciAddress};
	use memory_addresses::{PhysAddr, VirtAddr};
	use super::{PciConfigAccess, PCI_MAX_BUS_NUMBER};
	use crate::env;
	use crate::env::kernel::acpi;

	pub fn init_pcie() -> bool {
		let Some(table) = acpi::get_mcfg_table() else { return false; };

		let mut start_addr: *const McfgTableEntry = core::ptr::with_exposed_provenance(table.table_start_address() + 8);
		let end_addr: *const McfgTableEntry = core::ptr::with_exposed_provenance(table.table_end_address() + 8);

		if start_addr == end_addr {
			return false;
		}

		while start_addr < end_addr {
			unsafe {
				let read = ptr::read_unaligned(start_addr);
				init_pcie_bus(read);
				start_addr = start_addr.add(1);
			}
		}

		true
	}

	#[derive(Debug)]
	#[repr(C)]
	struct PcieDeviceConfig {
		vendor_id: u16,
		device_id: u16,
		_reserved: [u8; 4096 - 8]
	}

	#[derive(Debug, Copy, Clone)]
	#[repr(C)]
	pub(crate) struct McfgTableEntry {
		pub base_address: u64,
		pub pci_segment_number: u16,
		pub start_pci_bus: u8,
		pub end_pci_bus: u8,
		_reserved: u32
	}

	impl McfgTableEntry {
		pub fn pci_config_space_address(&self, bus_number: u8, device: u8, function: u8) -> PhysAddr {
			PhysAddr::new(
				self.base_address +
					((bus_number as u64) << 20) |
					(((device as u64) & 0x1f) << 15) |
					(((function as u64) & 0x7) << 12)
			)
		}
	}

	#[derive(Debug)]
	#[cfg(feature = "pci")]
	struct McfgTable(alloc::vec::Vec<McfgTableEntry>);

	impl PcieDeviceConfig {
		fn get<'a>(physical_address: PhysAddr) -> &'a Self {
			assert!(env::is_uefi());

			// For UEFI Systems, the tables are already mapped so we only need to return a proper reference to the table
			let allocated_virtual_address = VirtAddr::new(physical_address.as_u64());
			let ptr: *const PcieDeviceConfig = allocated_virtual_address.as_ptr();

			unsafe { ptr.as_ref().unwrap() }
		}
	}

	impl ConfigRegionAccess for McfgTableEntry {
		unsafe fn read(&self, address: PciAddress, offset: u16) -> u32 {
			assert_eq!(address.segment(), self.pci_segment_number);
			assert!(address.bus() >= self.start_pci_bus);
			assert!(address.bus() <= self.end_pci_bus);

			let ptr = self.pci_config_space_address(address.bus(), address.device(), address.function()) + offset as u64;
			let ptr = ptr.as_usize() as *const u32;

			unsafe { *ptr }
		}

		unsafe fn write(&self, address: PciAddress, offset: u16, value: u32) {
			assert_eq!(address.segment(), self.pci_segment_number);
			assert!(address.bus() >= self.start_pci_bus);
			assert!(address.bus() <= self.end_pci_bus);

			let ptr = self.pci_config_space_address(address.bus(), address.device(), address.function()) + offset as u64;
			let ptr = ptr.as_usize() as *mut u32;

			unsafe { *ptr = value; }
		}
	}

	fn init_pcie_bus(bus_entry: McfgTableEntry) {
		if bus_entry.start_pci_bus > PCI_MAX_BUS_NUMBER {
			return;
		}

		let end = if bus_entry.end_pci_bus > PCI_MAX_BUS_NUMBER { PCI_MAX_BUS_NUMBER } else { bus_entry.end_pci_bus };
		super::enumerate_devices(bus_entry.start_pci_bus, end, PciConfigAccess::PcieConfigRegion(bus_entry));
	}
}