use pci_types::{ConfigRegionAccess, PciAddress};

#[derive(Debug, Copy, Clone)]
pub struct PciConfigRegion;

impl ConfigRegionAccess for PciConfigRegion {
	unsafe fn read(&self, addr: PciAddress, offset: u16) -> u32 {
		warn!("pci_config_region.read({addr}, {offset}) called but not implemented");
		todo!()
	}

	unsafe fn write(&self, addr: PciAddress, offset: u16, value: u32) {
		warn!("pci_config_region.write({addr}, {offset}, {value}) called but not implemented");
	}
}
