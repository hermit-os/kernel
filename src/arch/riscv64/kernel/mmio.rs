use alloc::vec::Vec;

#[cfg(any(feature = "virtio-console", feature = "virtio-fs"))]
use hermit_sync::InterruptSpinMutex;

#[cfg(feature = "virtio-console")]
use crate::drivers::console::VirtioConsoleDriver;
#[cfg(feature = "virtio-fs")]
use crate::drivers::fs::VirtioFsDriver;
#[cfg(feature = "gem-net")]
use crate::drivers::net::gem::GEMDriver;
#[cfg(all(not(feature = "gem-net"), feature = "virtio-net"))]
use crate::drivers::net::virtio::VirtioNetDriver;
use crate::init_cell::InitCell;

pub(crate) static MMIO_DRIVERS: InitCell<Vec<MmioDriver>> = InitCell::new(Vec::new());

pub(crate) enum MmioDriver {
	#[cfg(feature = "virtio-console")]
	VirtioConsole(InterruptSpinMutex<VirtioConsoleDriver>),
	#[cfg(feature = "virtio-fs")]
	VirtioFs(InterruptSpinMutex<VirtioFsDriver>),
}

impl MmioDriver {
	#[cfg(feature = "virtio-console")]
	fn get_console_driver(&self) -> Option<&InterruptSpinMutex<VirtioConsoleDriver>> {
		match self {
			Self::VirtioConsole(drv) => Some(drv),
		}
	}

	#[cfg(feature = "virtio-fs")]
	fn get_filesystem_driver(&self) -> Option<&InterruptSpinMutex<VirtioFsDriver>> {
		match self {
			Self::VirtioFs(drv) => Some(drv),
		}
	}
}

#[cfg(any(feature = "virtio-console", feature = "virtio-fs"))]
pub(crate) fn register_driver(drv: MmioDriver) {
	MMIO_DRIVERS.with(|mmio_drivers| mmio_drivers.unwrap().push(drv));
}

#[cfg(feature = "gem-net")]
pub(crate) type NetworkDevice = GEMDriver;

#[cfg(all(not(feature = "gem-net"), feature = "virtio-net"))]
pub(crate) type NetworkDevice = VirtioNetDriver;

#[cfg(feature = "virtio-console")]
pub(crate) fn get_console_driver() -> Option<&'static InterruptSpinMutex<VirtioConsoleDriver>> {
	MMIO_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_console_driver())
}

#[cfg(feature = "virtio-fs")]
pub(crate) fn get_filesystem_driver() -> Option<&'static InterruptSpinMutex<VirtioFsDriver>> {
	MMIO_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_filesystem_driver())
}
