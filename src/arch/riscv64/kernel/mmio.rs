#![allow(dead_code)]

use alloc::vec::Vec;

use hermit_sync::InterruptSpinMutex;

#[cfg(feature = "console")]
use crate::drivers::console::VirtioConsoleDriver;
#[cfg(feature = "gem-net")]
use crate::drivers::net::gem::GEMDriver;
#[cfg(all(not(feature = "gem-net"), feature = "virtio-net"))]
use crate::drivers::net::virtio::VirtioNetDriver;
use crate::init_cell::InitCell;

pub(crate) static MMIO_DRIVERS: InitCell<Vec<MmioDriver>> = InitCell::new(Vec::new());

#[non_exhaustive]
pub(crate) enum MmioDriver {
	#[cfg(feature = "gem-net")]
	GEMNet(InterruptSpinMutex<GEMDriver>),
	#[cfg(all(not(feature = "gem-net"), feature = "virtio-net"))]
	VirtioNet(InterruptSpinMutex<VirtioNetDriver>),
	#[cfg(feature = "console")]
	VirtioConsole(InterruptSpinMutex<VirtioConsoleDriver>),
}

impl MmioDriver {
	#[cfg(feature = "gem-net")]
	fn get_network_driver(&self) -> Option<&InterruptSpinMutex<GEMDriver>> {
		#[allow(irrefutable_let_patterns)]
		if let Self::GEMNet(drv) = self {
			Some(drv)
		} else {
			None
		}
	}

	#[cfg(all(not(feature = "gem-net"), feature = "virtio-net"))]
	fn get_network_driver(&self) -> Option<&InterruptSpinMutex<VirtioNetDriver>> {
		#[allow(irrefutable_let_patterns)]
		if let Self::VirtioNet(drv) = self {
			Some(drv)
		} else {
			None
		}
	}

	#[cfg(feature = "console")]
	fn get_console_driver(&self) -> Option<&InterruptSpinMutex<VirtioConsoleDriver>> {
		#[allow(irrefutable_let_patterns)]
		if let Self::VirtioConsole(drv) = self {
			Some(drv)
		} else {
			None
		}
	}
}

pub(crate) fn register_driver(drv: MmioDriver) {
	MMIO_DRIVERS.with(|mmio_drivers| mmio_drivers.unwrap().push(drv));
}

#[cfg(feature = "gem-net")]
pub(crate) fn get_network_driver() -> Option<&'static InterruptSpinMutex<GEMDriver>> {
	MMIO_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_network_driver())
}

#[cfg(all(not(feature = "gem-net"), feature = "virtio-net"))]
pub(crate) fn get_network_driver() -> Option<&'static InterruptSpinMutex<VirtioNetDriver>> {
	MMIO_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_network_driver())
}

#[cfg(feature = "console")]
pub(crate) fn get_console_driver() -> Option<&'static InterruptSpinMutex<VirtioConsoleDriver>> {
	MMIO_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_console_driver())
}
