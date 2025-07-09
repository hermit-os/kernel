#![allow(dead_code)]

use alloc::vec::Vec;

use hermit_sync::InterruptSpinMutex;

#[cfg(feature = "console")]
use crate::drivers::console::VirtioConsoleDriver;
#[cfg(feature = "gem-net")]
use crate::drivers::net::gem::GEMDriver;
#[cfg(all(not(feature = "gem-net"), any(feature = "tcp", feature = "udp")))]
use crate::drivers::net::virtio::VirtioNetDriver;
use crate::init_cell::InitCell;

pub(crate) static MMIO_DRIVERS: InitCell<Vec<MmioDriver>> = InitCell::new(Vec::new());

pub(crate) enum MmioDriver {
	#[cfg(feature = "gem-net")]
	GEMNet(InterruptSpinMutex<GEMDriver>),
	#[cfg(all(not(feature = "gem-net"), any(feature = "tcp", feature = "udp")))]
	VirtioNet(InterruptSpinMutex<VirtioNetDriver>),
	#[cfg(feature = "console")]
	VirtioConsole(InterruptSpinMutex<VirtioConsoleDriver>),
}

impl MmioDriver {
	#[allow(unreachable_patterns)]
	#[cfg(feature = "gem-net")]
	fn get_network_driver(&self) -> Option<&InterruptSpinMutex<GEMDriver>> {
		match self {
			Self::GEMNet(drv) => Some(drv),
			_ => None,
		}
	}

	#[allow(unreachable_patterns)]
	#[cfg(all(not(feature = "gem-net"), any(feature = "tcp", feature = "udp")))]
	fn get_network_driver(&self) -> Option<&InterruptSpinMutex<VirtioNetDriver>> {
		match self {
			Self::VirtioNet(drv) => Some(drv),
			_ => None,
		}
	}

	#[cfg(feature = "console")]
	fn get_console_driver(&self) -> Option<&InterruptSpinMutex<VirtioConsoleDriver>> {
		match self {
			Self::VirtioConsole(drv) => Some(drv),
			#[cfg(any(feature = "tcp", feature = "udp"))]
			_ => None,
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

#[cfg(all(not(feature = "gem-net"), any(feature = "tcp", feature = "udp")))]
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
