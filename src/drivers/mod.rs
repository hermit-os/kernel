// Copyright (c) 2019 Stefan Lankes, RWTH Aachen University
//               2020 Frederik Schulz, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.
//
//! A module containing hermit-rs driver, hermit-rs driver trait and driver specific errors.
//!
//! The module contains ...

// !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
// UNCOMMENTED FOR CORRECT USE STATEMENT; IS THIS CORRECT?
// !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
//#[cfg(not(feature = "newlib"))]
pub mod net;

#[cfg(feature = "pci")]
pub mod virtio;

/// A common error module for drivers.
/// [DriverError](enums.drivererror.html) values will be
/// passed on to higher layers.
#[cfg(feature = "pci")]
pub mod error {
	use crate::drivers::net::rtl8139::RTL8139Error;
	use crate::drivers::virtio::error::VirtioError;
	use core::fmt;

	#[derive(Debug)]
	pub enum DriverError {
		InitVirtioDevFail(VirtioError),
		InitRTL8139DevFail(RTL8139Error),
	}

	impl From<VirtioError> for DriverError {
		fn from(err: VirtioError) -> Self {
			DriverError::InitVirtioDevFail(err)
		}
	}

	impl From<RTL8139Error> for DriverError {
		fn from(err: RTL8139Error) -> Self {
			DriverError::InitRTL8139DevFail(err)
		}
	}

	impl fmt::Display for DriverError {
		fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
			match *self {
				DriverError::InitVirtioDevFail(ref err) => {
					write!(f, "Virtio driver failed: {:?}", err)
				}
				DriverError::InitRTL8139DevFail(ref err) => {
					write!(f, "RTL8139 driver failed: {:?}", err)
				}
			}
		}
	}
}
