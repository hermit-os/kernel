use crate::drivers::net::gem::GEMDriver;
use crate::drivers::net::virtio_net::VirtioNetDriver;
use crate::drivers::net::NetworkInterface;
use crate::synch::spinlock::SpinlockIrqSave;
use alloc::vec::Vec;

static mut MMIO_DRIVERS: Vec<MmioDriver> = Vec::new();

pub enum MmioDriver {
	GEMNet(SpinlockIrqSave<GEMDriver>),
	VirtioNet(SpinlockIrqSave<VirtioNetDriver>),
}

impl<'a> MmioDriver {
	fn get_network_driver(&self) -> Option<&SpinlockIrqSave<dyn NetworkInterface>> {
		match self {
			Self::VirtioNet(drv) => Some(drv),
			Self::GEMNet(drv) => Some(drv),
			_ => None,
		}
	}
}
pub fn register_driver(drv: MmioDriver) {
	unsafe {
		MMIO_DRIVERS.push(drv);
	}
}

pub fn get_network_driver() -> Option<&'static SpinlockIrqSave<dyn NetworkInterface>> {
	unsafe { MMIO_DRIVERS.iter().find_map(|drv| drv.get_network_driver()) }
}
