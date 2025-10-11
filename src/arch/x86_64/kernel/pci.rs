use core::ops::Range;

use pci_types::{ConfigRegionAccess, PciAddress, PciHeader};
use x86_64::instructions::port::Port;

use crate::drivers::pci::{PCI_DEVICES, PciDevice};

const PCI_CONFIG_ADDRESS_ENABLE: u32 = 1 << 31;

const CONFIG_ADDRESS: Port<u32> = Port::new(0xcf8);
const CONFIG_DATA: Port<u32> = Port::new(0xcfc);

#[derive(Debug, Copy, Clone)]
pub enum PciConfigRegion {
	Pci(LegacyPciConfigRegion),
	#[cfg(feature = "acpi")]
	PciE(pcie::McfgEntry),
}

impl ConfigRegionAccess for PciConfigRegion {
	unsafe fn read(&self, address: PciAddress, offset: u16) -> u32 {
		match self {
			PciConfigRegion::Pci(entry) => unsafe { entry.read(address, offset) },
			#[cfg(feature = "acpi")]
			PciConfigRegion::PciE(entry) => unsafe { entry.read(address, offset) },
		}
	}

	unsafe fn write(&self, address: PciAddress, offset: u16, value: u32) {
		match self {
			PciConfigRegion::Pci(entry) => unsafe {
				entry.write(address, offset, value);
			},
			#[cfg(feature = "acpi")]
			PciConfigRegion::PciE(entry) => unsafe {
				entry.write(address, offset, value);
			},
		}
	}
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct LegacyPciConfigRegion;

impl LegacyPciConfigRegion {
	pub const fn new() -> Self {
		Self {}
	}
}

impl ConfigRegionAccess for LegacyPciConfigRegion {
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
	#[cfg(feature = "acpi")]
	if pcie::init_pcie() {
		info!("PCIe: Initialized");
		return;
	}

	// For Hermit, we currently limit scanning to the first 32 buses.
	const PCI_MAX_BUS_NUMBER: u8 = 32;
	scan_bus(
		0..PCI_MAX_BUS_NUMBER,
		PciConfigRegion::Pci(LegacyPciConfigRegion::new()),
	);
	info!("PCI: Initialized");
}

fn scan_bus(bus_range: Range<u8>, pci_config: PciConfigRegion) {
	info!("PCI: Scanning bus range {bus_range:?}");

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

#[cfg(feature = "acpi")]
mod pcie {
	use core::{ptr, slice};

	use memory_addresses::{PhysAddr, VirtAddr};
	use pci_types::{ConfigRegionAccess, PciAddress};

	use super::PciConfigRegion;
	use crate::arch::mm::paging::{
		self, LargePageSize, PageTableEntryFlags, PageTableEntryFlagsExt,
	};
	use crate::env::kernel::acpi;
	use crate::mm::device_alloc::DeviceAlloc;

	pub fn init_pcie() -> bool {
		let Some(table) = acpi::get_mcfg_table() else {
			return false;
		};

		let start = ptr::with_exposed_provenance::<McfgEntry>(table.table_start_address() + 8);
		let end = ptr::with_exposed_provenance::<McfgEntry>(table.table_end_address());
		let entries = unsafe { slice::from_ptr_range(start..end) };

		if entries.is_empty() {
			return false;
		}

		for entry in entries {
			init_pcie_bus(entry);
		}

		true
	}

	#[derive(Clone, Copy, Debug)]
	#[repr(C, packed)]
	pub struct McfgEntry {
		pub base_address: u64,
		pub pci_segment_group: u16,
		pub bus_number_start: u8,
		pub bus_number_end: u8,
		_reserved: u32,
	}

	impl McfgEntry {
		pub fn pci_config_space_address(
			&self,
			bus_number: u8,
			device: u8,
			function: u8,
		) -> PhysAddr {
			PhysAddr::new(
				self.base_address
					+ ((u64::from(bus_number) << 20)
						| ((u64::from(device) & 0x1f) << 15)
						| ((u64::from(function) & 0x7) << 12)),
			)
		}
	}

	impl ConfigRegionAccess for McfgEntry {
		unsafe fn read(&self, address: PciAddress, offset: u16) -> u32 {
			assert!(address.segment() == self.pci_segment_group);
			assert!(address.bus() >= self.bus_number_start);
			assert!(address.bus() <= self.bus_number_end);

			let phys_addr =
				self.pci_config_space_address(address.bus(), address.device(), address.function())
					+ u64::from(offset);
			let ptr = DeviceAlloc.ptr_from::<u32>(phys_addr);

			unsafe { ptr.read_volatile() }
		}

		unsafe fn write(&self, address: PciAddress, offset: u16, value: u32) {
			assert!(address.segment() == self.pci_segment_group);
			assert!(address.bus() >= self.bus_number_start);
			assert!(address.bus() <= self.bus_number_end);

			let phys_addr =
				self.pci_config_space_address(address.bus(), address.device(), address.function())
					+ u64::from(offset);
			let ptr = DeviceAlloc.ptr_from::<u32>(phys_addr);

			unsafe {
				ptr.write_volatile(value);
			}
		}
	}

	fn init_pcie_bus(bus_entry: &McfgEntry) {
		let phys_addr = PhysAddr::new(bus_entry.base_address);
		let virt_addr = VirtAddr::from_ptr(DeviceAlloc.ptr_from::<()>(phys_addr));
		if paging::virtual_to_physical(virt_addr) != Some(phys_addr) {
			debug!("Mapping PCIe memory");
			let flags = {
				let mut flags = PageTableEntryFlags::empty();
				flags.normal().writable().execute_disable();
				flags
			};
			paging::map::<LargePageSize>(
				virt_addr,
				phys_addr,
				bus_entry.bus_number_end.into(),
				flags,
			);
		}

		super::scan_bus(
			bus_entry.bus_number_start..bus_entry.bus_number_end,
			PciConfigRegion::PciE(*bus_entry),
		);
	}
}
