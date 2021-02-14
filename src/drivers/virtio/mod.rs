// Copyright (c) 2020 Frederik Schulz, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! A module containing virtios core infrastructure for hermit-rs.
//!
//! The module contains virtios transport mechanisms, virtqueues and virtio specific errors
pub mod depr;
pub mod env;
pub mod transport;
pub mod virtqueue;

pub mod error {
	use crate::arch::x86_64::kernel::pci::error::PciError;
	use crate::drivers::net::virtio_net::error::VirtioNetError;
	use core::fmt;

	#[derive(Debug)]
	pub enum VirtioError {
		FromPci(PciError),
		DevNotSupported(u16),
		NetDriver(VirtioNetError),
		Unknown,
	}

	impl fmt::Display for VirtioError {
		fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
			match self {
                VirtioError::Unknown =>write!(f, "Driver failed to initialize virtio device due to unknown reasosn!"),
                VirtioError::FromPci(pci_error) => match pci_error {
                    PciError::General(id) => write!(f, "Driver failed to initialize device with id: 0x{:x}. Due to unknown reasosn!", id),
                    PciError::NoBar(id ) => write!(f, "Driver failed to initialize device with id: 0x{:x}. Reason: No BAR's found.", id), 
                    PciError::NoCapPtr(id) => write!(f, "Driver failed to initialize device with id: 0x{:x}. Reason: No Capabilites pointer found.", id),
                    PciError::BadCapPtr(id) => write!(f, "Driver failed to initialize device with id: 0x{:x}. Reason: Malformed Capabilites pointer.", id),
                    PciError::NoBarForCap(id) => write!(f, "Driver failed to initialize device with id: 0x{:x}. Reason: Bar indicated by capability not found.", id),
                    PciError::NoVirtioCaps(id) => write!(f, "Driver failed to initialize device with id: 0x{:x}. Reason: No Virtio capabilites were found.", id),
                },
                VirtioError::DevNotSupported(id) => write!(f, "Devie with id 0x{:x} not supported.", id),
                VirtioError::NetDriver(net_error) => match net_error {
                    VirtioNetError::General => write!(f, "Virtio network driver failed due to unknown reasons!"),
                    VirtioNetError::NoDevCfg(id) => write!(f, "Network driver failed, for device {:x}, due to a missing or malformed device config!", id),
                    VirtioNetError::NoComCfg(id) =>  write!(f, "Network driver failed, for device {:x}, due to a missing or malformed common config!", id),
                    VirtioNetError::NoIsrCfg(id) =>  write!(f, "Network driver failed, for device {:x}, due to a missing or malformed ISR status config!", id),
                    VirtioNetError::NoNotifCfg(id) =>  write!(f, "Network driver failed, for device {:x}, due to a missing or malformed notification config!", id),
                    VirtioNetError::FailFeatureNeg(id) => write!(f, "Network driver failed, for device {:x}, device did not acknowledge negotiated feature set!", id),
                    VirtioNetError::FeatReqNotMet(feats) => write!(f, "Network driver tried to set feature bit without setting dependency feature. Feat set: {:x}", u64::from(*feats)),
                    VirtioNetError::IncompFeatsSet(drv_feats, dev_feats) => write!(f, "Feature set: {:x} , is incompatible with the device features: {:x}", u64::from(*drv_feats), u64::from(*dev_feats)),
                    VirtioNetError::ProcessOngoing => write!(f, "Driver performed an unsuitable operation upon an ongoging transfer."),
                },
            }
		}
	}
}

/// A module containing Virtio's feature bits.
pub mod features {
	use core::cmp::PartialEq;
	use core::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign};

	/// Virtio's feature bits inside an enum.
	/// See Virtio specification v1.1. - 6
	#[allow(dead_code, non_camel_case_types)]
	#[derive(Clone, Copy, Debug, PartialEq)]
	#[repr(u64)]
	pub enum Features {
		VIRTIO_F_RING_INDIRECT_DESC = 1 << 28,
		VIRTIO_F_RING_EVENT_IDX = 1 << 29,
		VIRTIO_F_VERSION_1 = 1 << 32,
		VIRTIO_F_ACCESS_PLATFORM = 1 << 33,
		VIRTIO_F_RING_PACKED = 1 << 34,
		VIRTIO_F_IN_ORDER = 1 << 35,
		VIRTIO_F_ORDER_PLATFORM = 1 << 36,
		VIRTIO_F_SR_IOV = 1 << 37,
		VIRTIO_F_NOTIFICATION_DATA = 1 << 38,
	}

	impl PartialEq<Features> for u64 {
		fn eq(&self, other: &Features) -> bool {
			*self == u64::from(*other)
		}
	}

	impl PartialEq<u64> for Features {
		fn eq(&self, other: &u64) -> bool {
			u64::from(*self) == *other
		}
	}

	impl From<Features> for u64 {
		fn from(val: Features) -> Self {
			match val {
				Features::VIRTIO_F_RING_INDIRECT_DESC => 1 << 28,
				Features::VIRTIO_F_RING_EVENT_IDX => 1 << 29,
				Features::VIRTIO_F_VERSION_1 => 1 << 32,
				Features::VIRTIO_F_ACCESS_PLATFORM => 1 << 33,
				Features::VIRTIO_F_RING_PACKED => 1 << 34,
				Features::VIRTIO_F_IN_ORDER => 1 << 35,
				Features::VIRTIO_F_ORDER_PLATFORM => 1 << 36,
				Features::VIRTIO_F_SR_IOV => 1 << 37,
				Features::VIRTIO_F_NOTIFICATION_DATA => 1 << 38,
			}
		}
	}

	impl BitOr for Features {
		type Output = u64;

		fn bitor(self, rhs: Self) -> Self::Output {
			u64::from(self) | u64::from(rhs)
		}
	}

	impl BitOr<Features> for u64 {
		type Output = u64;

		fn bitor(self, rhs: Features) -> Self::Output {
			self | u64::from(rhs)
		}
	}

	impl BitOrAssign<Features> for u64 {
		fn bitor_assign(&mut self, rhs: Features) {
			*self |= u64::from(rhs);
		}
	}

	impl BitAnd for Features {
		type Output = u64;

		fn bitand(self, rhs: Features) -> Self::Output {
			u64::from(self) & u64::from(rhs)
		}
	}

	impl BitAnd<Features> for u64 {
		type Output = u64;

		fn bitand(self, rhs: Features) -> Self::Output {
			self & u64::from(rhs)
		}
	}

	impl BitAndAssign<Features> for u64 {
		fn bitand_assign(&mut self, rhs: Features) {
			*self &= u64::from(rhs);
		}
	}
}

/// A module containing virtios device specfific information.
pub mod device {
	/// An enum of the device's status field interpretations.
	#[allow(dead_code, non_camel_case_types)]
	#[derive(Clone, Copy, Debug)]
	#[repr(u8)]
	pub enum Status {
		ACKNOWLEDGE = 1,
		DRIVER = 2,
		DRIVER_OK = 4,
		FEATURES_OK = 8,
		DEVICE_NEEDS_RESET = 64,
		FAILED = 128,
	}

	impl From<Status> for u8 {
		fn from(stat: Status) -> Self {
			match stat {
				Status::ACKNOWLEDGE => 1,
				Status::DRIVER => 2,
				Status::DRIVER_OK => 4,
				Status::FEATURES_OK => 8,
				Status::DEVICE_NEEDS_RESET => 64,
				Status::FAILED => 128,
			}
		}
	}

	/// Empty trait to unify all device specific configuration structs.
	pub trait DevCfg {}
}
