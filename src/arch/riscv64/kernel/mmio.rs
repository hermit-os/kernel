use alloc::vec::Vec;

#[cfg(any(
	feature = "virtio-console",
	feature = "virtio-fs",
	feature = "virtio-vsock",
))]
use hermit_sync::InterruptSpinMutex;

#[cfg(feature = "virtio-console")]
use crate::drivers::console::VirtioConsoleDriver;
#[cfg(feature = "virtio-fs")]
use crate::drivers::fs::VirtioFsDriver;
#[cfg(feature = "gem-net")]
use crate::drivers::net::gem::GEMDriver;
#[cfg(all(not(feature = "gem-net"), feature = "virtio-net"))]
use crate::drivers::net::virtio::VirtioNetDriver;
#[cfg(feature = "virtio-vsock")]
use crate::drivers::vsock::VirtioVsockDriver;
use crate::init_cell::InitCell;

pub(crate) static MMIO_DRIVERS: InitCell<Vec<MmioDriver>> = InitCell::new(Vec::new());

#[allow(clippy::enum_variant_names)]
pub(crate) enum MmioDriver {
	#[cfg(feature = "virtio-console")]
	VirtioConsole(InterruptSpinMutex<VirtioConsoleDriver>),
	#[cfg(feature = "virtio-fs")]
	VirtioFs(InterruptSpinMutex<VirtioFsDriver>),
	#[cfg(feature = "virtio-vsock")]
	VirtioVsock(InterruptSpinMutex<VirtioVsockDriver>),
}

impl MmioDriver {
	#[cfg(feature = "virtio-console")]
	fn get_console_driver(&self) -> Option<&InterruptSpinMutex<VirtioConsoleDriver>> {
		#[allow(unreachable_patterns)]
		match self {
			Self::VirtioConsole(drv) => Some(drv),
			_ => None,
		}
	}

	#[cfg(feature = "virtio-fs")]
	fn get_filesystem_driver(&self) -> Option<&InterruptSpinMutex<VirtioFsDriver>> {
		#[allow(unreachable_patterns)]
		match self {
			Self::VirtioFs(drv) => Some(drv),
			_ => None,
		}
	}

	#[cfg(feature = "virtio-vsock")]
	fn get_vsock_driver(&self) -> Option<&InterruptSpinMutex<VirtioVsockDriver>> {
		#[allow(unreachable_patterns)]
		match self {
			Self::VirtioVsock(drv) => Some(drv),
			_ => None,
		}
	}
}

#[cfg(any(
	feature = "virtio-console",
	feature = "virtio-fs",
	feature = "virtio-vsock",
))]
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

#[cfg(feature = "virtio-vsock")]
pub(crate) fn get_vsock_driver() -> Option<&'static InterruptSpinMutex<VirtioVsockDriver>> {
	MMIO_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_vsock_driver())
}
