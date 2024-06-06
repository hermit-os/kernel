use pci_types::{ConfigRegionAccess, PciAddress, PciHeader};
use x86::io::*;

use crate::drivers::pci::{PciDevice, PCI_DEVICES};

const PCI_MAX_BUS_NUMBER: u8 = 32;
const PCI_MAX_DEVICE_NUMBER: u8 = 32;

const PCI_CONFIG_ADDRESS_PORT: u16 = 0xCF8;
const PCI_CONFIG_ADDRESS_ENABLE: u32 = 1 << 31;

const PCI_CONFIG_DATA_PORT: u16 = 0xCFC;

#[derive(Debug, Copy, Clone)]
pub(crate) struct PciConfigRegion;

impl PciConfigRegion {
	pub const fn new() -> Self {
		Self {}
	}
}

impl ConfigRegionAccess for PciConfigRegion {
	#[inline]
	fn function_exists(&self, _address: PciAddress) -> bool {
		true
	}

	#[inline]
	unsafe fn read(&self, pci_addr: PciAddress, register: u16) -> u32 {
		let address = PCI_CONFIG_ADDRESS_ENABLE
			| u32::from(pci_addr.bus()) << 16
			| u32::from(pci_addr.device()) << 11
			| u32::from(pci_addr.function()) << 8
			| u32::from(register);
		unsafe {
			outl(PCI_CONFIG_ADDRESS_PORT, address);
			u32::from_le(inl(PCI_CONFIG_DATA_PORT))
		}
	}

	#[inline]
	unsafe fn write(&self, pci_addr: PciAddress, register: u16, value: u32) {
		let address = PCI_CONFIG_ADDRESS_ENABLE
			| u32::from(pci_addr.bus()) << 16
			| u32::from(pci_addr.device()) << 11
			| u32::from(pci_addr.function()) << 8
			| u32::from(register);
		unsafe {
			outl(PCI_CONFIG_ADDRESS_PORT, address);
			outl(PCI_CONFIG_DATA_PORT, value.to_le());
		}
	}
}

pub(crate) fn init() {
	debug!("Scanning PCI Busses 0 to {}", PCI_MAX_BUS_NUMBER - 1);

	// Hermit only uses PCI for network devices.
	// Therefore, multifunction devices as well as additional bridges are not scanned.
	// We also limit scanning to the first 32 buses.
	let pci_config = PciConfigRegion::new();
	for bus in 0..PCI_MAX_BUS_NUMBER {
		for device in 0..PCI_MAX_DEVICE_NUMBER {
			let pci_address = PciAddress::new(0, bus, device, 0);
			let header = PciHeader::new(pci_address);

			let (device_id, vendor_id) = header.id(&pci_config);
			if device_id != u16::MAX && vendor_id != u16::MAX {
				unsafe {
					PCI_DEVICES.push(PciDevice::new(pci_address, pci_config));
				}
			}
		}
	}
}
