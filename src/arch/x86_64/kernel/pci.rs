use core::ops::Range;

use pci_types::{ConfigRegionAccess, PciAddress, PciHeader};
use x86_64::instructions::port::Port;

use crate::drivers::pci::{PCI_DEVICES, PciDevice};

const PCI_CONFIG_ADDRESS_ENABLE: u32 = 1 << 31;

const CONFIG_ADDRESS: Port<u32> = Port::new(0xcf8);
const CONFIG_DATA: Port<u32> = Port::new(0xcfc);

#[derive(Debug, Copy, Clone)]
pub(crate) struct PciConfigRegion;

impl PciConfigRegion {
	pub const fn new() -> Self {
		Self {}
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
	// For Hermit, we currently limit scanning to the first 32 buses.
	const PCI_MAX_BUS_NUMBER: u8 = 32;
	scan_bus(0..PCI_MAX_BUS_NUMBER, PciConfigRegion::new());
	info!("Initialized PCI");
}

fn scan_bus(bus_range: Range<u8>, pci_config: PciConfigRegion) {
	debug!("Scanning PCI buses {bus_range:?}");

	// Hermit only uses PCI for network devices.
	// Therefore, multifunction devices as well as additional bridges are not scanned.
	for bus in bus_range {
		// For Hermit, we currently limit scanning to the first 32 devices.
		const PCI_MAX_DEVICE_NUMBER: u8 = 32;
		for device in 0..PCI_MAX_DEVICE_NUMBER {
			let pci_address = PciAddress::new(0, bus, device, 0);
			let header = PciHeader::new(pci_address);

			let (device_id, vendor_id) = header.id(pci_config);
			if device_id != u16::MAX && vendor_id != u16::MAX {
				let device = PciDevice::new(pci_address, pci_config);
				PCI_DEVICES.with(|pci_devices| pci_devices.unwrap().push(device));
			}
		}
	}
}
