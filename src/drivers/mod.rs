//! A module containing hermit-rs driver, hermit-rs driver trait and driver specific errors.

#[cfg(feature = "fuse")]
pub mod fs;
#[cfg(not(feature = "pci"))]
pub mod mmio;
#[cfg(any(feature = "tcp", feature = "udp"))]
pub mod net;
#[cfg(feature = "pci")]
pub mod pci;
#[cfg(any(
	all(any(feature = "tcp", feature = "udp"), not(feature = "rtl8139")),
	feature = "fuse",
	feature = "vsock"
))]
pub mod virtio;
#[cfg(feature = "vsock")]
pub mod vsock;

use alloc::collections::VecDeque;

#[cfg(feature = "pci")]
pub(crate) use pci_types::InterruptLine;
#[cfg(not(feature = "pci"))]
pub(crate) type InterruptLine = u8;

pub(crate) type InterruptHandlerQueue = VecDeque<fn()>;

/// A common error module for drivers.
/// [DriverError](error::DriverError) values will be
/// passed on to higher layers.
pub mod error {
	#[cfg(all(target_arch = "riscv64", feature = "gem-net"))]
	use crate::drivers::net::gem::GEMError;
	#[cfg(all(target_arch = "x86_64", feature = "rtl8139"))]
	use crate::drivers::net::rtl8139::RTL8139Error;
	#[cfg(any(
		all(any(feature = "tcp", feature = "udp"), not(feature = "rtl8139")),
		feature = "fuse",
		feature = "vsock"
	))]
	use crate::drivers::virtio::error::VirtioError;

	#[cfg(any(feature = "tcp", feature = "udp", feature = "fuse", feature = "vsock"))]
	#[derive(Debug)]
	pub enum DriverError {
		#[cfg(any(
			all(any(feature = "tcp", feature = "udp"), not(feature = "rtl8139")),
			feature = "fuse",
			feature = "vsock"
		))]
		InitVirtioDevFail(VirtioError),
		#[cfg(all(target_arch = "x86_64", feature = "rtl8139"))]
		InitRTL8139DevFail(RTL8139Error),
		#[cfg(all(target_arch = "riscv64", feature = "gem-net"))]
		InitGEMDevFail(GEMError),
	}

	#[cfg(any(
		all(any(feature = "tcp", feature = "udp"), not(feature = "rtl8139")),
		feature = "fuse",
		feature = "vsock"
	))]
	impl From<VirtioError> for DriverError {
		fn from(err: VirtioError) -> Self {
			DriverError::InitVirtioDevFail(err)
		}
	}

	#[cfg(all(target_arch = "x86_64", feature = "rtl8139"))]
	impl From<RTL8139Error> for DriverError {
		fn from(err: RTL8139Error) -> Self {
			DriverError::InitRTL8139DevFail(err)
		}
	}

	#[cfg(all(target_arch = "riscv64", feature = "gem-net"))]
	impl From<GEMError> for DriverError {
		fn from(err: GEMError) -> Self {
			DriverError::InitGEMDevFail(err)
		}
	}

	#[cfg(any(feature = "tcp", feature = "udp", feature = "fuse", feature = "vsock"))]
	impl core::fmt::Display for DriverError {
		#[allow(unused_variables)]
		fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
			match *self {
				#[cfg(any(
					all(any(feature = "tcp", feature = "udp"), not(feature = "rtl8139")),
					feature = "fuse",
					feature = "vsock"
				))]
				DriverError::InitVirtioDevFail(ref err) => {
					write!(f, "Virtio driver failed: {err:?}")
				}
				#[cfg(all(target_arch = "x86_64", feature = "rtl8139"))]
				DriverError::InitRTL8139DevFail(ref err) => {
					write!(f, "RTL8139 driver failed: {err:?}")
				}
				#[cfg(all(target_arch = "riscv64", feature = "gem-net"))]
				DriverError::InitGEMDevFail(ref err) => {
					write!(f, "GEM driver failed: {err:?}")
				}
			}
		}
	}
}

/// A trait to determine general driver information
#[allow(dead_code)]
pub(crate) trait Driver {
	/// Returns the interrupt number of the device
	fn get_interrupt_number(&self) -> InterruptLine;

	/// Returns the device driver name
	fn get_name(&self) -> &'static str;
}

pub(crate) fn init() {
	// Initialize PCI Drivers
	#[cfg(feature = "pci")]
	crate::drivers::pci::init();
	#[cfg(all(
		not(feature = "pci"),
		target_arch = "x86_64",
		any(feature = "tcp", feature = "udp")
	))]
	crate::arch::x86_64::kernel::mmio::init_drivers();

	#[cfg(target_arch = "riscv64")]
	crate::arch::riscv64::kernel::init_drivers();

	crate::arch::interrupts::install_handlers();
}
