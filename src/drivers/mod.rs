//! A module containing hermit-rs driver, hermit-rs driver trait and driver specific errors.

#[cfg(feature = "fs")]
pub mod fs;
#[cfg(not(feature = "pci"))]
pub mod mmio;
#[cfg(any(feature = "tcp", feature = "udp"))]
pub mod net;
#[cfg(feature = "pci")]
pub mod pci;
#[cfg(any(
	all(any(feature = "tcp", feature = "udp"), not(feature = "rtl8139")),
	feature = "fs"
))]
pub mod virtio;

/// A common error module for drivers.
/// [DriverError](enums.drivererror.html) values will be
/// passed on to higher layers.
pub mod error {
	use core::fmt;

	#[cfg(feature = "gem-net")]
	use crate::drivers::net::gem::GEMError;
	#[cfg(feature = "rtl8139")]
	use crate::drivers::net::rtl8139::RTL8139Error;
	#[cfg(any(
		all(any(feature = "tcp", feature = "udp"), not(feature = "rtl8139")),
		feature = "fs"
	))]
	use crate::drivers::virtio::error::VirtioError;

	#[derive(Debug)]
	pub enum DriverError {
		#[cfg(any(
			all(any(feature = "tcp", feature = "udp"), not(feature = "rtl8139")),
			feature = "fs"
		))]
		InitVirtioDevFail(VirtioError),
		#[cfg(feature = "rtl8139")]
		InitRTL8139DevFail(RTL8139Error),
		#[cfg(feature = "gem-net")]
		InitGEMDevFail(GEMError),
	}

	#[cfg(any(
		all(any(feature = "tcp", feature = "udp"), not(feature = "rtl8139")),
		feature = "fs"
	))]
	impl From<VirtioError> for DriverError {
		fn from(err: VirtioError) -> Self {
			DriverError::InitVirtioDevFail(err)
		}
	}

	#[cfg(feature = "rtl8139")]
	impl From<RTL8139Error> for DriverError {
		fn from(err: RTL8139Error) -> Self {
			DriverError::InitRTL8139DevFail(err)
		}
	}

	#[cfg(feature = "gem-net")]
	impl From<GEMError> for DriverError {
		fn from(err: GEMError) -> Self {
			DriverError::InitGEMDevFail(err)
		}
	}

	impl fmt::Display for DriverError {
		#[allow(unused_variables)]
		fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
			match *self {
				#[cfg(any(
					all(any(feature = "tcp", feature = "udp"), not(feature = "rtl8139")),
					feature = "fs"
				))]
				DriverError::InitVirtioDevFail(ref err) => {
					write!(f, "Virtio driver failed: {err:?}")
				}
				#[cfg(feature = "rtl8139")]
				DriverError::InitRTL8139DevFail(ref err) => {
					write!(f, "RTL8139 driver failed: {err:?}")
				}
				#[cfg(feature = "gem-net")]
				DriverError::InitGEMDevFail(ref err) => {
					write!(f, "GEM driver failed: {err:?}")
				}
			}
		}
	}
}
