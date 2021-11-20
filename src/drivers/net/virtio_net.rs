//! A module containing a virtio network driver.
//!
//! The module contains ...

#[cfg(not(feature = "newlib"))]
use super::netwakeup;
use crate::arch::kernel::percore::increment_irq_counter;
use crate::config::VIRTIO_MAX_QUEUE_SIZE;
use crate::drivers::net::NetworkInterface;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::convert::TryFrom;
use core::mem;
use core::result::Result;
use core::{cell::RefCell, cmp::Ordering};

use crate::drivers::virtio::error::VirtioError;
#[cfg(not(feature = "pci"))]
use crate::drivers::virtio::transport::mmio;
#[cfg(not(feature = "pci"))]
use crate::drivers::virtio::transport::mmio::{ComCfg, IsrStatus, NotifCfg};
#[cfg(feature = "pci")]
use crate::drivers::virtio::transport::pci;
#[cfg(feature = "pci")]
use crate::drivers::virtio::transport::pci::{ComCfg, IsrStatus, NotifCfg, PciCap, UniCapsColl};
use crate::drivers::virtio::virtqueue::{
	AsSliceU8, BuffSpec, BufferToken, Bytes, Transfer, Virtq, VqIndex, VqSize, VqType,
};

use self::constants::{FeatureSet, Features, NetHdrGSO, Status, MAX_NUM_VQ};
use self::error::VirtioNetError;

pub const ETH_HDR: usize = 14usize;

#[derive(Debug)]
#[repr(C)]
pub struct VirtioNetHdr {
	flags: u8,
	gso_type: u8,
	/// Ethernet + IP + tcp/udp hdrs
	hdr_len: u16,
	/// Bytes to append to hdr_len per frame
	gso_size: u16,
	/// Position to start checksumming from
	csum_start: u16,
	/// Offset after that to place checksum
	csum_offset: u16,
	/// Number of buffers this Packet consists of
	num_buffers: u16,
}

// Using the default implementation of the trait for VirtioNetHdr
impl AsSliceU8 for VirtioNetHdr {}

impl VirtioNetHdr {
	pub fn get_tx_hdr() -> VirtioNetHdr {
		VirtioNetHdr {
			flags: 0,
			gso_type: NetHdrGSO::VIRTIO_NET_HDR_GSO_NONE.into(),
			hdr_len: 0,
			gso_size: 0,
			csum_start: 0,
			csum_offset: 0,
			num_buffers: 0,
		}
	}

	pub fn get_rx_hdr() -> VirtioNetHdr {
		VirtioNetHdr {
			flags: 0,
			gso_type: 0,
			hdr_len: 0,
			gso_size: 0,
			csum_start: 0,
			csum_offset: 0,
			num_buffers: 0,
		}
	}
}

pub mod constants {
	pub use super::error::VirtioNetError;
	use alloc::vec::Vec;
	use core::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign};

	// Configuration constants
	pub const MAX_NUM_VQ: u16 = 2;

	/// Enum containing Virtios netword header flags
	///
	/// See Virtio specification v1.1. - 5.1.6
	#[allow(dead_code, non_camel_case_types)]
	#[derive(Copy, Clone, Debug)]
	#[repr(u8)]
	///
	pub enum NetHdrFlag {
		/// use csum_start, csum_offset
		VIRTIO_NET_HDR_F_NEEDS_CSUM = 1,
		/// csum is valid
		VIRTIO_NET_HDR_F_DATA_VALID = 2,
		/// reports number of coalesced TCP segments
		VIRTIO_NET_HDR_F_RSC_INFO = 4,
	}

	impl From<NetHdrFlag> for u8 {
		fn from(val: NetHdrFlag) -> Self {
			match val {
				NetHdrFlag::VIRTIO_NET_HDR_F_NEEDS_CSUM => 1,
				NetHdrFlag::VIRTIO_NET_HDR_F_DATA_VALID => 2,
				NetHdrFlag::VIRTIO_NET_HDR_F_RSC_INFO => 4,
			}
		}
	}

	impl BitOr for NetHdrFlag {
		type Output = u8;

		fn bitor(self, rhs: Self) -> Self::Output {
			u8::from(self) | u8::from(rhs)
		}
	}

	impl BitOr<NetHdrFlag> for u8 {
		type Output = u8;

		fn bitor(self, rhs: NetHdrFlag) -> Self::Output {
			self | u8::from(rhs)
		}
	}

	impl BitOrAssign<NetHdrFlag> for u8 {
		fn bitor_assign(&mut self, rhs: NetHdrFlag) {
			*self |= u8::from(rhs);
		}
	}

	impl BitAnd for NetHdrFlag {
		type Output = u8;

		fn bitand(self, rhs: NetHdrFlag) -> Self::Output {
			u8::from(self) & u8::from(rhs)
		}
	}

	impl BitAnd<NetHdrFlag> for u8 {
		type Output = u8;

		fn bitand(self, rhs: NetHdrFlag) -> Self::Output {
			self & u8::from(rhs)
		}
	}

	impl BitAndAssign<NetHdrFlag> for u8 {
		fn bitand_assign(&mut self, rhs: NetHdrFlag) {
			*self &= u8::from(rhs);
		}
	}

	/// Enum containing Virtios netword GSO types
	///
	/// See Virtio specification v1.1. - 5.1.6
	#[allow(dead_code, non_camel_case_types)]
	#[derive(Copy, Clone, Debug)]
	#[repr(u8)]
	pub enum NetHdrGSO {
		/// not a GSO frame
		VIRTIO_NET_HDR_GSO_NONE = 0,
		/// GSO frame, IPv4 TCP (TSO)
		VIRTIO_NET_HDR_GSO_TCPV4 = 1,
		/// GSO frame, IPv4 UDP (UFO)
		VIRTIO_NET_HDR_GSO_UDP = 3,
		/// GSO frame, IPv6 TCP
		VIRTIO_NET_HDR_GSO_TCPV6 = 4,
		/// TCP has ECN set
		VIRTIO_NET_HDR_GSO_ECN = 0x80,
	}

	impl From<NetHdrGSO> for u8 {
		fn from(val: NetHdrGSO) -> Self {
			match val {
				NetHdrGSO::VIRTIO_NET_HDR_GSO_NONE => 0,
				NetHdrGSO::VIRTIO_NET_HDR_GSO_TCPV4 => 1,
				NetHdrGSO::VIRTIO_NET_HDR_GSO_UDP => 3,
				NetHdrGSO::VIRTIO_NET_HDR_GSO_TCPV6 => 4,
				NetHdrGSO::VIRTIO_NET_HDR_GSO_ECN => 0x80,
			}
		}
	}

	impl BitOr for NetHdrGSO {
		type Output = u8;

		fn bitor(self, rhs: Self) -> Self::Output {
			u8::from(self) | u8::from(rhs)
		}
	}

	impl BitOr<NetHdrGSO> for u8 {
		type Output = u8;

		fn bitor(self, rhs: NetHdrGSO) -> Self::Output {
			self | u8::from(rhs)
		}
	}

	impl BitOrAssign<NetHdrGSO> for u8 {
		fn bitor_assign(&mut self, rhs: NetHdrGSO) {
			*self |= u8::from(rhs);
		}
	}

	impl BitAnd for NetHdrGSO {
		type Output = u8;

		fn bitand(self, rhs: NetHdrGSO) -> Self::Output {
			u8::from(self) & u8::from(rhs)
		}
	}

	impl BitAnd<NetHdrGSO> for u8 {
		type Output = u8;

		fn bitand(self, rhs: NetHdrGSO) -> Self::Output {
			self & u8::from(rhs)
		}
	}

	impl BitAndAssign<NetHdrGSO> for u8 {
		fn bitand_assign(&mut self, rhs: NetHdrGSO) {
			*self &= u8::from(rhs);
		}
	}

	/// Enum contains virtio's network device features and general features of Virtio.
	///
	/// See Virtio specification v1.1. - 5.1.3
	///
	/// See Virtio specification v1.1. - 6
	//
	// WARN: In case the enum is changed, the static function of features `into_features(feat: u64) ->
	// Option<Vec<Features>>` must also be adjusted to return a corret vector of features.
	#[allow(dead_code, non_camel_case_types)]
	#[derive(Copy, Clone, Debug)]
	#[repr(u64)]
	pub enum Features {
		VIRTIO_NET_F_CSUM = 1 << 0,
		VIRTIO_NET_F_GUEST_CSUM = 1 << 1,
		VIRTIO_NET_F_CTRL_GUEST_OFFLOADS = 1 << 2,
		VIRTIO_NET_F_MTU = 1 << 3,
		VIRTIO_NET_F_MAC = 1 << 5,
		VIRTIO_NET_F_GUEST_TSO4 = 1 << 7,
		VIRTIO_NET_F_GUEST_TSO6 = 1 << 8,
		VIRTIO_NET_F_GUEST_ECN = 1 << 9,
		VIRTIO_NET_F_GUEST_UFO = 1 << 10,
		VIRTIO_NET_F_HOST_TSO4 = 1 << 11,
		VIRTIO_NET_F_HOST_TSO6 = 1 << 12,
		VIRTIO_NET_F_HOST_ECN = 1 << 13,
		VIRTIO_NET_F_HOST_UFO = 1 << 14,
		VIRTIO_NET_F_MRG_RXBUF = 1 << 15,
		VIRTIO_NET_F_STATUS = 1 << 16,
		VIRTIO_NET_F_CTRL_VQ = 1 << 17,
		VIRTIO_NET_F_CTRL_RX = 1 << 18,
		VIRTIO_NET_F_CTRL_VLAN = 1 << 19,
		VIRTIO_NET_F_GUEST_ANNOUNCE = 1 << 21,
		VIRTIO_NET_F_MQ = 1 << 22,
		VIRTIO_NET_F_CTRL_MAC_ADDR = 1 << 23,
		VIRTIO_F_RING_INDIRECT_DESC = 1 << 28,
		VIRTIO_F_RING_EVENT_IDX = 1 << 29,
		VIRTIO_F_VERSION_1 = 1 << 32,
		VIRTIO_F_ACCESS_PLATFORM = 1 << 33,
		VIRTIO_F_RING_PACKED = 1 << 34,
		VIRTIO_F_IN_ORDER = 1 << 35,
		VIRTIO_F_ORDER_PLATFORM = 1 << 36,
		VIRTIO_F_SR_IOV = 1 << 37,
		VIRTIO_F_NOTIFICATION_DATA = 1 << 38,
		VIRTIO_NET_F_GUEST_HDRLEN = 1 << 59,
		VIRTIO_NET_F_RSC_EXT = 1 << 61,
		VIRTIO_NET_F_STANDBY = 1 << 62,
		// INTERNAL DOCUMENTATION TO KNOW WHICH FEATURES HAVE REQUIREMENTS
		//
		// 5.1.3.1 Feature bit requirements
		// Some networking feature bits require other networking feature bits (see 2.2.1):
		// VIRTIO_NET_F_GUEST_TSO4 Requires VIRTIO_NET_F_GUEST_CSUM.
		// VIRTIO_NET_F_GUEST_TSO6 Requires VIRTIO_NET_F_GUEST_CSUM.
		// VIRTIO_NET_F_GUEST_ECN Requires VIRTIO_NET_F_GUEST_TSO4orVIRTIO_NET_F_GUEST_TSO6.
		// VIRTIO_NET_F_GUEST_UFO Requires VIRTIO_NET_F_GUEST_CSUM.
		// VIRTIO_NET_F_HOST_TSO4 Requires VIRTIO_NET_F_CSUM.
		// VIRTIO_NET_F_HOST_TSO6 Requires VIRTIO_NET_F_CSUM.
		// VIRTIO_NET_F_HOST_ECN Requires VIRTIO_NET_F_HOST_TSO4 or VIRTIO_NET_F_HOST_TSO6.
		// VIRTIO_NET_F_HOST_UFO Requires VIRTIO_NET_F_CSUM.
		// VIRTIO_NET_F_CTRL_RX Requires VIRTIO_NET_F_CTRL_VQ.
		// VIRTIO_NET_F_CTRL_VLAN Requires VIRTIO_NET_F_CTRL_VQ.
		// VIRTIO_NET_F_GUEST_ANNOUNCE Requires VIRTIO_NET_F_CTRL_VQ.
		// VIRTIO_NET_F_MQ Requires VIRTIO_NET_F_CTRL_VQ.
		// VIRTIO_NET_F_CTRL_MAC_ADDR Requires VIRTIO_NET_F_CTRL_VQ.
		// VIRTIO_NET_F_RSC_EXT Requires VIRTIO_NET_F_HOST_TSO4 or VIRTIO_NET_F_HOST_TSO6.
	}

	impl From<Features> for u64 {
		fn from(val: Features) -> Self {
			match val {
				Features::VIRTIO_NET_F_CSUM => 1 << 0,
				Features::VIRTIO_NET_F_GUEST_CSUM => 1 << 1,
				Features::VIRTIO_NET_F_CTRL_GUEST_OFFLOADS => 1 << 2,
				Features::VIRTIO_NET_F_MTU => 1 << 3,
				Features::VIRTIO_NET_F_MAC => 1 << 5,
				Features::VIRTIO_NET_F_GUEST_TSO4 => 1 << 7,
				Features::VIRTIO_NET_F_GUEST_TSO6 => 1 << 8,
				Features::VIRTIO_NET_F_GUEST_ECN => 1 << 9,
				Features::VIRTIO_NET_F_GUEST_UFO => 1 << 10,
				Features::VIRTIO_NET_F_HOST_TSO4 => 1 << 11,
				Features::VIRTIO_NET_F_HOST_TSO6 => 1 << 12,
				Features::VIRTIO_NET_F_HOST_ECN => 1 << 13,
				Features::VIRTIO_NET_F_HOST_UFO => 1 << 14,
				Features::VIRTIO_NET_F_MRG_RXBUF => 1 << 15,
				Features::VIRTIO_NET_F_STATUS => 1 << 16,
				Features::VIRTIO_NET_F_CTRL_VQ => 1 << 17,
				Features::VIRTIO_NET_F_CTRL_RX => 1 << 18,
				Features::VIRTIO_NET_F_CTRL_VLAN => 1 << 19,
				Features::VIRTIO_NET_F_GUEST_ANNOUNCE => 1 << 21,
				Features::VIRTIO_NET_F_MQ => 1 << 22,
				Features::VIRTIO_NET_F_CTRL_MAC_ADDR => 1 << 23,
				Features::VIRTIO_F_RING_INDIRECT_DESC => 1 << 28,
				Features::VIRTIO_F_RING_EVENT_IDX => 1 << 29,
				Features::VIRTIO_F_VERSION_1 => 1 << 32,
				Features::VIRTIO_F_ACCESS_PLATFORM => 1 << 33,
				Features::VIRTIO_F_RING_PACKED => 1 << 34,
				Features::VIRTIO_F_IN_ORDER => 1 << 35,
				Features::VIRTIO_F_ORDER_PLATFORM => 1 << 36,
				Features::VIRTIO_F_SR_IOV => 1 << 37,
				Features::VIRTIO_F_NOTIFICATION_DATA => 1 << 38,
				Features::VIRTIO_NET_F_GUEST_HDRLEN => 1 << 59,
				Features::VIRTIO_NET_F_RSC_EXT => 1 << 61,
				Features::VIRTIO_NET_F_STANDBY => 1 << 62,
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

	impl core::fmt::Display for Features {
		fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
			match *self {
				Features::VIRTIO_NET_F_CSUM => write!(f, "VIRTIO_NET_F_CSUM"),
				Features::VIRTIO_NET_F_GUEST_CSUM => write!(f, "VIRTIO_NET_F_GUEST_CSUM"),
				Features::VIRTIO_NET_F_CTRL_GUEST_OFFLOADS => {
					write!(f, "VIRTIO_NET_F_CTRL_GUEST_OFFLOADS")
				}
				Features::VIRTIO_NET_F_MTU => write!(f, "VIRTIO_NET_F_MTU"),
				Features::VIRTIO_NET_F_MAC => write!(f, "VIRTIO_NET_F_MAC"),
				Features::VIRTIO_NET_F_GUEST_TSO4 => write!(f, "VIRTIO_NET_F_GUEST_TSO4"),
				Features::VIRTIO_NET_F_GUEST_TSO6 => write!(f, "VIRTIO_NET_F_GUEST_TSO6"),
				Features::VIRTIO_NET_F_GUEST_ECN => write!(f, "VIRTIO_NET_F_GUEST_ECN"),
				Features::VIRTIO_NET_F_GUEST_UFO => write!(f, "VIRTIO_NET_FGUEST_UFO"),
				Features::VIRTIO_NET_F_HOST_TSO4 => write!(f, "VIRTIO_NET_F_HOST_TSO4"),
				Features::VIRTIO_NET_F_HOST_TSO6 => write!(f, "VIRTIO_NET_F_HOST_TSO6"),
				Features::VIRTIO_NET_F_HOST_ECN => write!(f, "VIRTIO_NET_F_HOST_ECN"),
				Features::VIRTIO_NET_F_HOST_UFO => write!(f, "VIRTIO_NET_F_HOST_UFO"),
				Features::VIRTIO_NET_F_MRG_RXBUF => write!(f, "VIRTIO_NET_F_MRG_RXBUF"),
				Features::VIRTIO_NET_F_STATUS => write!(f, "VIRTIO_NET_F_STATUS"),
				Features::VIRTIO_NET_F_CTRL_VQ => write!(f, "VIRTIO_NET_F_CTRL_VQ"),
				Features::VIRTIO_NET_F_CTRL_RX => write!(f, "VIRTIO_NET_F_CTRL_RX"),
				Features::VIRTIO_NET_F_CTRL_VLAN => write!(f, "VIRTIO_NET_F_CTRL_VLAN"),
				Features::VIRTIO_NET_F_GUEST_ANNOUNCE => write!(f, "VIRTIO_NET_F_GUEST_ANNOUNCE"),
				Features::VIRTIO_NET_F_MQ => write!(f, "VIRTIO_NET_F_MQ"),
				Features::VIRTIO_NET_F_CTRL_MAC_ADDR => write!(f, "VIRTIO_NET_F_CTRL_MAC_ADDR"),
				Features::VIRTIO_F_RING_INDIRECT_DESC => write!(f, "VIRTIO_F_RING_INDIRECT_DESC"),
				Features::VIRTIO_F_RING_EVENT_IDX => write!(f, "VIRTIO_F_RING_EVENT_IDX"),
				Features::VIRTIO_F_VERSION_1 => write!(f, "VIRTIO_F_VERSION_1"),
				Features::VIRTIO_F_ACCESS_PLATFORM => write!(f, "VIRTIO_F_ACCESS_PLATFORM"),
				Features::VIRTIO_F_RING_PACKED => write!(f, "VIRTIO_F_RING_PACKED"),
				Features::VIRTIO_F_IN_ORDER => write!(f, "VIRTIO_F_IN_ORDER"),
				Features::VIRTIO_F_ORDER_PLATFORM => write!(f, "VIRTIO_F_ORDER_PLATFORM"),
				Features::VIRTIO_F_SR_IOV => write!(f, "VIRTIO_F_SR_IOV"),
				Features::VIRTIO_F_NOTIFICATION_DATA => write!(f, "VIRTIO_F_NOTIFICATION_DATA"),
				Features::VIRTIO_NET_F_GUEST_HDRLEN => write!(f, "VIRTIO_NET_F_GUEST_HDRLEN"),
				Features::VIRTIO_NET_F_RSC_EXT => write!(f, "VIRTIO_NET_F_RSC_EXT"),
				Features::VIRTIO_NET_F_STANDBY => write!(f, "VIRTIO_NET_F_STANDBY"),
			}
		}
	}

	impl Features {
		/// Return a vector of [Features](Features) for a given input of a u64 representation.
		///
		/// INFO: In case the FEATURES enum is changed, this function MUST also be adjusted to the new set!
		//
		// Really UGLY function, but currently the most convenienvt one to reduce the set of features for the driver easily!
		pub fn from_set(feat_set: FeatureSet) -> Option<Vec<Features>> {
			let mut vec_of_feats: Vec<Features> = Vec::new();
			let feats = feat_set.0;

			if feats & (1 << 0) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_CSUM)
			}
			if feats & (1 << 1) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_GUEST_CSUM)
			}
			if feats & (1 << 2) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_CTRL_GUEST_OFFLOADS)
			}
			if feats & (1 << 3) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_MTU)
			}
			if feats & (1 << 5) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_MAC)
			}
			if feats & (1 << 7) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_GUEST_TSO4)
			}
			if feats & (1 << 8) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_GUEST_TSO6)
			}
			if feats & (1 << 9) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_GUEST_ECN)
			}
			if feats & (1 << 10) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_GUEST_UFO)
			}
			if feats & (1 << 11) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_HOST_TSO4)
			}
			if feats & (1 << 12) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_HOST_TSO6)
			}
			if feats & (1 << 13) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_HOST_ECN)
			}
			if feats & (1 << 14) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_HOST_UFO)
			}
			if feats & (1 << 15) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_MRG_RXBUF)
			}
			if feats & (1 << 16) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_STATUS)
			}
			if feats & (1 << 17) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_CTRL_VQ)
			}
			if feats & (1 << 18) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_CTRL_RX)
			}
			if feats & (1 << 19) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_CTRL_VLAN)
			}
			if feats & (1 << 21) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_GUEST_ANNOUNCE)
			}
			if feats & (1 << 22) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_MQ)
			}
			if feats & (1 << 23) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_CTRL_MAC_ADDR)
			}
			if feats & (1 << 28) != 0 {
				vec_of_feats.push(Features::VIRTIO_F_RING_INDIRECT_DESC)
			}
			if feats & (1 << 29) != 0 {
				vec_of_feats.push(Features::VIRTIO_F_RING_EVENT_IDX)
			}
			if feats & (1 << 32) != 0 {
				vec_of_feats.push(Features::VIRTIO_F_VERSION_1)
			}
			if feats & (1 << 33) != 0 {
				vec_of_feats.push(Features::VIRTIO_F_ACCESS_PLATFORM)
			}
			if feats & (1 << 34) != 0 {
				vec_of_feats.push(Features::VIRTIO_F_RING_PACKED)
			}
			if feats & (1 << 35) != 0 {
				vec_of_feats.push(Features::VIRTIO_F_IN_ORDER)
			}
			if feats & (1 << 36) != 0 {
				vec_of_feats.push(Features::VIRTIO_F_ORDER_PLATFORM)
			}
			if feats & (1 << 37) != 0 {
				vec_of_feats.push(Features::VIRTIO_F_SR_IOV)
			}
			if feats & (1 << 38) != 0 {
				vec_of_feats.push(Features::VIRTIO_F_NOTIFICATION_DATA)
			}
			if feats & (1 << 59) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_GUEST_HDRLEN)
			}
			if feats & (1 << 61) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_RSC_EXT)
			}
			if feats & (1 << 62) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_STANDBY)
			}

			if vec_of_feats.is_empty() {
				None
			} else {
				Some(vec_of_feats)
			}
		}
	}

	/// Enum contains virtio's network device status
	/// indiacted in the status field of the device's
	/// configuration structure.
	///
	/// See Virtio specification v1.1. - 5.1.4
	#[allow(dead_code, non_camel_case_types)]
	#[derive(Copy, Clone, Debug)]
	#[repr(u16)]
	pub enum Status {
		VIRTIO_NET_S_LINK_UP = 1 << 0,
		VIRTIO_NET_S_ANNOUNCE = 1 << 1,
	}

	impl From<Status> for u16 {
		fn from(stat: Status) -> Self {
			match stat {
				Status::VIRTIO_NET_S_LINK_UP => 1,
				Status::VIRTIO_NET_S_ANNOUNCE => 2,
			}
		}
	}

	/// FeatureSet is new type whicih holds features for virito network devices indicated by the virtio specification
	/// v1.1. - 5.1.3. and all General Features defined in Virtio specification v1.1. - 6
	/// wrapping a u64.
	///
	/// The main functionality of this type are functions implemented on it.
	#[derive(Debug, Copy, Clone, PartialOrd, PartialEq)]
	pub struct FeatureSet(u64);

	impl BitOr for FeatureSet {
		type Output = FeatureSet;

		fn bitor(self, rhs: Self) -> Self::Output {
			FeatureSet(self.0 | rhs.0)
		}
	}

	impl BitOr<FeatureSet> for u64 {
		type Output = u64;

		fn bitor(self, rhs: FeatureSet) -> Self::Output {
			self | u64::from(rhs)
		}
	}

	impl BitOrAssign<FeatureSet> for u64 {
		fn bitor_assign(&mut self, rhs: FeatureSet) {
			*self |= u64::from(rhs);
		}
	}

	impl BitOrAssign<Features> for FeatureSet {
		fn bitor_assign(&mut self, rhs: Features) {
			self.0 = self.0 | u64::from(rhs);
		}
	}

	impl BitAnd for FeatureSet {
		type Output = FeatureSet;

		fn bitand(self, rhs: FeatureSet) -> Self::Output {
			FeatureSet(self.0 & rhs.0)
		}
	}

	impl BitAnd<FeatureSet> for u64 {
		type Output = u64;

		fn bitand(self, rhs: FeatureSet) -> Self::Output {
			self & u64::from(rhs)
		}
	}

	impl BitAndAssign<FeatureSet> for u64 {
		fn bitand_assign(&mut self, rhs: FeatureSet) {
			*self &= u64::from(rhs);
		}
	}

	impl From<FeatureSet> for u64 {
		fn from(feature_set: FeatureSet) -> Self {
			feature_set.0
		}
	}

	impl FeatureSet {
		/// Checks if a given set of features is compatible and adheres to the
		/// specfification v1.1. - 5.1.3.1
		/// Upon an error returns the incompatible set of features by the
		/// [FeatReqNotMet](self::error::VirtioNetError) errror value, which
		/// wraps the u64 indicating the feature set.
		///
		/// INFO: Iterates twice over the vector of features.
		pub fn check_features(feats: &[Features]) -> Result<(), VirtioNetError> {
			let mut feat_bits = 0u64;

			for feat in feats.iter() {
				feat_bits |= *feat;
			}

			for feat in feats {
				match feat {
					Features::VIRTIO_NET_F_CSUM => continue,
					Features::VIRTIO_NET_F_GUEST_CSUM => continue,
					Features::VIRTIO_NET_F_CTRL_GUEST_OFFLOADS => continue,
					Features::VIRTIO_NET_F_MTU => continue,
					Features::VIRTIO_NET_F_MAC => continue,
					Features::VIRTIO_NET_F_GUEST_TSO4 => {
						if feat_bits & Features::VIRTIO_NET_F_GUEST_CSUM != 0 {
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_GUEST_TSO6 => {
						if feat_bits & Features::VIRTIO_NET_F_GUEST_CSUM != 0 {
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_GUEST_ECN => {
						if feat_bits
							& (Features::VIRTIO_NET_F_GUEST_TSO4
								| Features::VIRTIO_NET_F_GUEST_TSO6)
							!= 0
						{
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_GUEST_UFO => {
						if feat_bits & Features::VIRTIO_NET_F_GUEST_CSUM != 0 {
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_HOST_TSO4 => {
						if feat_bits & Features::VIRTIO_NET_F_CSUM != 0 {
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_HOST_TSO6 => {
						if feat_bits & Features::VIRTIO_NET_F_CSUM != 0 {
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_HOST_ECN => {
						if feat_bits
							& (Features::VIRTIO_NET_F_HOST_TSO4 | Features::VIRTIO_NET_F_HOST_TSO6)
							!= 0
						{
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_HOST_UFO => {
						if feat_bits & Features::VIRTIO_NET_F_CSUM != 0 {
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_MRG_RXBUF => continue,
					Features::VIRTIO_NET_F_STATUS => continue,
					Features::VIRTIO_NET_F_CTRL_VQ => continue,
					Features::VIRTIO_NET_F_CTRL_RX => {
						if feat_bits & Features::VIRTIO_NET_F_CTRL_VQ != 0 {
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_CTRL_VLAN => {
						if feat_bits & Features::VIRTIO_NET_F_CTRL_VQ != 0 {
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_GUEST_ANNOUNCE => {
						if feat_bits & Features::VIRTIO_NET_F_CTRL_VQ != 0 {
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_MQ => {
						if feat_bits & Features::VIRTIO_NET_F_CTRL_VQ != 0 {
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_CTRL_MAC_ADDR => {
						if feat_bits & Features::VIRTIO_NET_F_CTRL_VQ != 0 {
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_GUEST_HDRLEN => continue,
					Features::VIRTIO_NET_F_RSC_EXT => {
						if feat_bits
							& (Features::VIRTIO_NET_F_HOST_TSO4 | Features::VIRTIO_NET_F_HOST_TSO6)
							!= 0
						{
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_STANDBY => continue,
					Features::VIRTIO_F_RING_INDIRECT_DESC => continue,
					Features::VIRTIO_F_RING_EVENT_IDX => continue,
					Features::VIRTIO_F_VERSION_1 => continue,
					Features::VIRTIO_F_ACCESS_PLATFORM => continue,
					Features::VIRTIO_F_RING_PACKED => continue,
					Features::VIRTIO_F_IN_ORDER => continue,
					Features::VIRTIO_F_ORDER_PLATFORM => continue,
					Features::VIRTIO_F_SR_IOV => continue,
					Features::VIRTIO_F_NOTIFICATION_DATA => continue,
				}
			}

			Ok(())
		}

		/// Checks if a given feature is set.
		pub fn is_feature(self, feat: Features) -> bool {
			self.0 & feat != 0
		}

		/// Sets features contained in feats to true.
		///
		/// WARN: Features should be checked before using this function via the [`check_features`] function.
		pub fn set_features(&mut self, feats: &[Features]) {
			for feat in feats {
				self.0 |= *feat;
			}
		}

		/// Returns a new instance of (FeatureSet)[FeatureSet] with all features
		/// initialized to false.
		pub fn new(val: u64) -> Self {
			FeatureSet(val)
		}
	}
}

/// Error module of virtios network driver. Containing the (VirtioNetError)[VirtioNetError]
/// enum.
pub mod error {
	use super::constants::FeatureSet;
	/// Network drivers error enum.
	#[derive(Debug, Copy, Clone)]
	pub enum VirtioNetError {
		General,
		NoDevCfg(u16),
		NoComCfg(u16),
		NoIsrCfg(u16),
		NoNotifCfg(u16),
		FailFeatureNeg(u16),
		/// Set of features does not adhere to the requirements of features
		/// indicated by the specification
		FeatReqNotMet(FeatureSet),
		/// The first u64 contains the feature bits wanted by the driver.
		/// but which are incompatible with the device feature set, second u64.
		IncompFeatsSet(FeatureSet, FeatureSet),
		/// Indicates that an operation for finished Transfers, was performed on
		/// an ongoing transfer
		ProcessOngoing,
		Unknown,
	}
}
