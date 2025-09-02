#![allow(dead_code)]

use alloc::vec::Vec;

#[cfg(feature = "console")]
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
	#[cfg(feature = "console")]
	VirtioConsole(InterruptSpinMutex<VirtioConsoleDriver>),
}

impl MmioDriver {
	#[cfg(feature = "console")]
	fn get_console_driver(&self) -> Option<&InterruptSpinMutex<VirtioConsoleDriver>> {
		match self {
			Self::VirtioConsole(drv) => Some(drv),
		}
	}
}

pub(crate) fn register_driver(drv: MmioDriver) {
	MMIO_DRIVERS.with(|mmio_drivers| mmio_drivers.unwrap().push(drv));
}

#[cfg(feature = "gem-net")]
pub(crate) type NetworkDevice = GEMDriver;

#[cfg(all(not(feature = "gem-net"), any(feature = "tcp", feature = "udp")))]
pub(crate) type NetworkDevice = VirtioNetDriver;

#[cfg(feature = "console")]
pub(crate) fn get_console_driver() -> Option<&'static InterruptSpinMutex<VirtioConsoleDriver>> {
	MMIO_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_console_driver())
}
