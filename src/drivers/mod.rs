//! A module containing hermit-rs driver, hermit-rs driver trait and driver specific errors.

#[cfg(feature = "virtio-console")]
pub mod console;
#[cfg(feature = "virtio-fs")]
pub mod fs;
#[cfg(not(feature = "pci"))]
pub mod mmio;
#[cfg(feature = "net")]
pub mod net;
#[cfg(feature = "pci")]
pub mod pci;
#[cfg(feature = "virtio")]
pub mod virtio;
#[cfg(feature = "virtio-vsock")]
pub mod vsock;

use alloc::collections::VecDeque;

use ahash::RandomState;
use hashbrown::HashMap;
#[cfg(feature = "pci")]
pub(crate) use pci_types::InterruptLine;
#[cfg(not(feature = "pci"))]
pub(crate) type InterruptLine = u8;

pub(crate) type InterruptHandlerQueue = VecDeque<fn()>;

/// A common error module for drivers.
/// [DriverError](error::DriverError) values will be
/// passed on to higher layers.
pub mod error {
	#[cfg(any(
		feature = "virtio",
		all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci")),
		feature = "rtl8139",
	))]
	use thiserror::Error;

	#[cfg(all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci")))]
	use crate::drivers::net::gem::GEMError;
	#[cfg(feature = "rtl8139")]
	use crate::drivers::net::rtl8139::RTL8139Error;
	#[cfg(feature = "virtio")]
	use crate::drivers::virtio::error::VirtioError;

	#[cfg(any(
		feature = "virtio",
		all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci")),
		feature = "rtl8139",
	))]
	#[derive(Error, Debug)]
	pub enum DriverError {
		#[cfg(feature = "virtio")]
		#[error("Virtio driver failed: {0:?}")]
		InitVirtioDevFail(#[from] VirtioError),

		#[cfg(feature = "rtl8139")]
		#[error("RTL8139 driver failed: {0:?}")]
		InitRTL8139DevFail(#[from] RTL8139Error),

		#[cfg(all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci")))]
		#[error("GEM driver failed: {0:?}")]
		InitGEMDevFail(#[from] GEMError),
	}
}

/// A trait to determine general driver information
#[allow(dead_code)]
pub(crate) trait Driver {
	/// Returns the device driver name
	fn get_name(&self) -> &'static str;
}

pub(crate) fn init() {
	#[cfg_attr(
		all(
			not(feature = "pci"),
			not(target_arch = "riscv64"),
			not(feature = "virtio")
		),
		expect(unused_mut)
	)]
	let mut handlers = HashMap::with_hasher(RandomState::with_seeds(0, 0, 0, 0));

	// Initialize PCI Drivers
	#[cfg(feature = "pci")]
	pci::init(&mut handlers);
	#[cfg(all(not(feature = "pci"), feature = "virtio", target_arch = "x86_64"))]
	crate::arch::x86_64::kernel::mmio::init_drivers(&mut handlers);
	#[cfg(all(not(feature = "pci"), feature = "virtio", target_arch = "aarch64"))]
	crate::arch::aarch64::kernel::mmio::init_drivers(&mut handlers);

	#[cfg(target_arch = "riscv64")]
	crate::arch::riscv64::kernel::init_drivers(&mut handlers);

	crate::arch::interrupts::install_handlers(handlers);
}
