use alloc::vec::Vec;

use hermit_sync::InterruptSpinMutex;

#[cfg(feature = "gem-net")]
use crate::drivers::net::gem::GEMDriver;
#[cfg(not(feature = "gem-net"))]
use crate::drivers::net::virtio_net::VirtioNetDriver;

static mut MMIO_DRIVERS: Vec<MmioDriver> = Vec::new();

pub(crate) enum MmioDriver {
	#[cfg(feature = "gem-net")]
	GEMNet(InterruptSpinMutex<GEMDriver>),
	#[cfg(not(feature = "gem-net"))]
	VirtioNet(InterruptSpinMutex<VirtioNetDriver>),
}

impl MmioDriver {
	#[cfg(feature = "gem-net")]
	fn get_network_driver(&self) -> Option<&InterruptSpinMutex<GEMDriver>> {
		match self {
			Self::GEMNet(drv) => Some(drv),
		}
	}

	#[cfg(not(feature = "gem-net"))]
	fn get_network_driver(&self) -> Option<&InterruptSpinMutex<VirtioNetDriver>> {
		match self {
			Self::VirtioNet(drv) => Some(drv),
		}
	}
}
pub(crate) fn register_driver(drv: MmioDriver) {
	unsafe {
		MMIO_DRIVERS.push(drv);
	}
}

#[cfg(feature = "gem-net")]
pub(crate) fn get_network_driver() -> Option<&'static InterruptSpinMutex<GEMDriver>> {
	unsafe { MMIO_DRIVERS.iter().find_map(|drv| drv.get_network_driver()) }
}

#[cfg(not(feature = "gem-net"))]
pub(crate) fn get_network_driver() -> Option<&'static InterruptSpinMutex<VirtioNetDriver>> {
	unsafe { MMIO_DRIVERS.iter().find_map(|drv| drv.get_network_driver()) }
}
