use alloc::vec::Vec;
use core::{str, u32, u64, u8};

use hermit_dtb::Dtb;
use pci_types::{ConfigRegionAccess, PciAddress, PciHeader};

use crate::arch::aarch64::mm::paging::{self, BasePageSize, PageSize, PageTableEntryFlags};
use crate::arch::aarch64::mm::{virtualmem, PhysAddr, VirtAddr};
use crate::drivers::pci::{PciDevice, PCI_DEVICES};
use crate::kernel::boot_info;

const PCI_MAX_DEVICE_NUMBER: u8 = 32;
const PCI_MAX_FUNCTION_NUMBER: u8 = 8;

#[derive(Debug, Copy, Clone)]
pub(crate) struct PciConfigRegion(VirtAddr);

impl PciConfigRegion {
	pub const fn new(addr: VirtAddr) -> Self {
		assert!(addr.as_u64() & 0xFFFFFFF == 0, "Unaligend PCI Config Space");
		Self(addr)
	}
}

impl ConfigRegionAccess for PciConfigRegion {
	#[inline]
	fn function_exists(&self, address: PciAddress) -> bool {
		// we trust the device tree
		true
	}

	#[inline]
	unsafe fn read(&self, pci_addr: PciAddress, offset: u16) -> u32 {
		assert!(offset & 0xF000 == 0, "Inavlid offset");
		let addr = u64::from(pci_addr.bus()) << 20
			| u64::from(pci_addr.device()) << 15
			| u64::from(pci_addr.function()) << 12
			| (u64::from(offset) & 0xFFF)
			| self.0.as_u64();
		crate::drivers::pci::from_pci_endian(core::ptr::read_volatile(addr as *const u32))
	}

	#[inline]
	unsafe fn write(&self, pci_addr: PciAddress, offset: u16, value: u32) {
		assert!(offset & 0xF000 == 0, "Inavlid offset");
		let addr = u64::from(pci_addr.bus()) << 20
			| u64::from(pci_addr.device()) << 15
			| u64::from(pci_addr.function()) << 12
			| (u64::from(offset) & 0xFFF)
			| self.0.as_u64();
		core::ptr::write_volatile(addr as *mut u32, value.to_le());
	}
}

pub fn init() {
	let dtb = unsafe {
		Dtb::from_raw(boot_info().hardware_info.device_tree.unwrap().get() as *const u8)
			.expect(".dtb file has invalid header")
	};

	for node in dtb.enum_subnodes("/") {
		let parts: Vec<_> = node.split('@').collect();

		if let Some(compatible) = dtb.get_property(parts.first().unwrap(), "compatible") {
			if str::from_utf8(compatible)
				.unwrap()
				.find("pci-host-ecam-generic")
				.is_some()
			{
				let reg = dtb.get_property(parts.first().unwrap(), "reg").unwrap();
				let (slice, residual_slice) = reg.split_at(core::mem::size_of::<u64>());
				let addr = PhysAddr(u64::from_be_bytes(slice.try_into().unwrap()));
				let (slice, residual_slice) = residual_slice.split_at(core::mem::size_of::<u64>());
				let size = u64::from_be_bytes(slice.try_into().unwrap());

				let pci_address =
					virtualmem::allocate_aligned(size.try_into().unwrap(), 0x10000000).unwrap();
				info!("Mapping PCI Enhanced Configuration Space interface to virtual address {:#X} (size {:#X})", pci_address, size);

				let mut flags = PageTableEntryFlags::empty();
				flags.device().writable().execute_disable();
				paging::map::<BasePageSize>(
					pci_address,
					addr,
					(size / BasePageSize::SIZE).try_into().unwrap(),
					flags,
				);

				let max_bus_number = size
					/ (PCI_MAX_DEVICE_NUMBER as u64
						* PCI_MAX_FUNCTION_NUMBER as u64
						* BasePageSize::SIZE);
				info!("Scanning PCI Busses 0 to {}", max_bus_number - 1);

				let pci_config = PciConfigRegion::new(pci_address);
				for bus in 0..max_bus_number {
					for device in 0..PCI_MAX_DEVICE_NUMBER {
						let pci_address = PciAddress::new(0, bus.try_into().unwrap(), device, 0);
						let header = PciHeader::new(pci_address);

						let (device_id, vendor_id) = header.id(&pci_config);
						if device_id != u16::MAX && vendor_id != u16::MAX {
							unsafe {
								PCI_DEVICES.push(PciDevice::new(pci_address, pci_config));
							}
						}
					}
				}

				return;
			} else if str::from_utf8(compatible)
				.unwrap()
				.find("pci-host-cam-generic")
				.is_some()
			{
				warn!("Currently, pci-host-cam-generic isn't supported!");
			}
		}
	}

	warn!("Unable to find PCI bus");
}
