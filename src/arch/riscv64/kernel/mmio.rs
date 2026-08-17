use alloc::vec::Vec;

#[cfg(any(
	feature = "virtio-blk",
	feature = "virtio-fs",
	feature = "virtio-rng",
	feature = "virtio-vsock",
))]
use hermit_sync::InterruptSpinMutex;

#[cfg(feature = "virtio-blk")]
use crate::drivers::blk::VirtioBlkDriver;
#[cfg(feature = "virtio-fs")]
use crate::drivers::fs::VirtioFsDriver;
#[cfg(feature = "gem-net")]
use crate::drivers::net::gem::GEMDriver;
#[cfg(all(not(feature = "gem-net"), feature = "virtio-net"))]
use crate::drivers::net::virtio::VirtioNetDriver;
#[cfg(feature = "virtio-rng")]
use crate::drivers::rng::VirtioRngDriver;
#[cfg(feature = "virtio-vsock")]
use crate::drivers::vsock::VirtioVsockDriver;
use crate::init_cell::InitCell;

pub(crate) static MMIO_DRIVERS: InitCell<Vec<MmioDriver>> = InitCell::new(Vec::new());

#[allow(clippy::enum_variant_names, clippy::large_enum_variant)]
#[non_exhaustive]
pub(crate) enum MmioDriver {
	#[cfg(feature = "virtio-blk")]
	VirtioBlk(InterruptSpinMutex<VirtioBlkDriver>),
	#[cfg(feature = "virtio-fs")]
	VirtioFs(InterruptSpinMutex<VirtioFsDriver>),
	#[cfg(feature = "virtio-rng")]
	VirtioRng(InterruptSpinMutex<VirtioRngDriver>),
	#[cfg(feature = "virtio-vsock")]
	VirtioVsock(InterruptSpinMutex<VirtioVsockDriver>),
}

impl MmioDriver {
	#[cfg(feature = "virtio-blk")]
	fn get_block_driver(&self) -> Option<&InterruptSpinMutex<VirtioBlkDriver>> {
		#[allow(unreachable_patterns)]
		match self {
			Self::VirtioBlk(drv) => Some(drv),
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

	#[cfg(feature = "virtio-rng")]
	fn get_rng_driver(&self) -> Option<&InterruptSpinMutex<VirtioRngDriver>> {
		#[allow(unreachable_patterns)]
		match self {
			Self::VirtioRng(drv) => Some(drv),
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
	feature = "virtio-blk",
	feature = "virtio-fs",
	feature = "virtio-rng",
	feature = "virtio-vsock",
))]
pub(crate) fn register_driver(drv: MmioDriver) {
	MMIO_DRIVERS.with(|mmio_drivers| mmio_drivers.unwrap().push(drv));
}

#[cfg(feature = "gem-net")]
pub(crate) type NetworkDevice = GEMDriver;

#[cfg(all(not(feature = "gem-net"), feature = "virtio-net"))]
pub(crate) type NetworkDevice = VirtioNetDriver;

#[cfg(feature = "virtio-fs")]
pub(crate) fn get_filesystem_driver() -> Option<&'static InterruptSpinMutex<VirtioFsDriver>> {
	MMIO_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_filesystem_driver())
}

#[cfg(feature = "virtio-blk")]
pub(crate) fn get_block_driver() -> Option<&'static InterruptSpinMutex<VirtioBlkDriver>> {
	MMIO_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_block_driver())
}

#[cfg(feature = "virtio-rng")]
pub(crate) fn get_rng_driver() -> Option<&'static InterruptSpinMutex<VirtioRngDriver>> {
	MMIO_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_rng_driver())
}

#[cfg(feature = "virtio-vsock")]
pub(crate) fn get_vsock_driver() -> Option<&'static InterruptSpinMutex<VirtioVsockDriver>> {
	MMIO_DRIVERS
		.get()?
		.iter()
		.find_map(|drv| drv.get_vsock_driver())
}
