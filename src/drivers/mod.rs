//! A module containing hermit-rs driver, hermit-rs driver trait and driver specific errors.

pub mod fs;
pub mod net;
#[cfg(feature = "pci")]
pub mod pci;
pub mod virtio;

/// A common error module for drivers.
/// [DriverError](enums.drivererror.html) values will be
/// passed on to higher layers.
pub mod error {
	use core::fmt;

	#[cfg(all(feature = "pci", not(target_arch = "aarch64")))]
	use crate::drivers::net::rtl8139::RTL8139Error;
	use crate::drivers::virtio::error::VirtioError;

	#[derive(Debug)]
	pub enum DriverError {
		InitVirtioDevFail(VirtioError),
		#[cfg(all(feature = "pci", not(target_arch = "aarch64")))]
		InitRTL8139DevFail(RTL8139Error),
	}

	impl From<VirtioError> for DriverError {
		fn from(err: VirtioError) -> Self {
			DriverError::InitVirtioDevFail(err)
		}
	}

	#[cfg(all(feature = "pci", not(target_arch = "aarch64")))]
	impl From<RTL8139Error> for DriverError {
		fn from(err: RTL8139Error) -> Self {
			DriverError::InitRTL8139DevFail(err)
		}
	}

	impl fmt::Display for DriverError {
		fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
			match *self {
				DriverError::InitVirtioDevFail(ref err) => {
					write!(f, "Virtio driver failed: {err:?}")
				}
				#[cfg(all(feature = "pci", not(target_arch = "aarch64")))]
				DriverError::InitRTL8139DevFail(ref err) => {
					write!(f, "RTL8139 driver failed: {err:?}")
				}
			}
		}
	}
}
