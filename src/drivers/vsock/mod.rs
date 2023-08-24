#[cfg(feature = "pci")]
pub mod pci;

use alloc::rc::Rc;
use alloc::vec::Vec;

use crate::config::VIRTIO_MAX_QUEUE_SIZE;
use crate::drivers::virtio::error::VirtioVsockError;
#[cfg(feature = "pci")]
use crate::drivers::virtio::transport::pci::{ComCfg, IsrStatus, NotifCfg};
use crate::drivers::virtio::virtqueue::{Virtq, VqIndex, VqSize, VqType};
use crate::drivers::vsock::constants::{FeatureSet, Features};
#[cfg(feature = "pci")]
use crate::drivers::vsock::pci::VsockDevCfgRaw;

#[derive(Debug)]
#[repr(C)]
struct VirtioVsockHdr {
	src_cid: u64,
	dst_cid: u64,
	src_port: u32,
	dst_port: u32,
	len: u32,
	type_: u16,
	op: u16,
	flags: u32,
	buf_alloc: u32,
	fwd_cnt: u32,
}

/// A wrapper struct for the raw configuration structure.
/// Handling the right access to fields, as some are read-only
/// for the driver.
pub(crate) struct VsockDevCfg {
	pub raw: &'static VsockDevCfgRaw,
	pub dev_id: u16,
	pub features: FeatureSet,
}

pub(crate) struct VirtioVsockDriver {
	pub(super) dev_cfg: VsockDevCfg,
	pub(super) com_cfg: ComCfg,
	pub(super) isr_stat: IsrStatus,
	pub(super) notif_cfg: NotifCfg,
	pub(super) vqueues: Vec<Rc<Virtq>>,
}

impl VirtioVsockDriver {
	#[cfg(feature = "pci")]
	pub fn get_dev_id(&self) -> u16 {
		self.dev_cfg.dev_id
	}

	#[cfg(feature = "pci")]
	pub fn set_failed(&mut self) {
		self.com_cfg.set_failed();
	}

	/// Negotiates a subset of features, understood and wanted by both the OS
	/// and the device.
	fn negotiate_features(&mut self, wanted_feats: &[Features]) -> Result<(), VirtioVsockError> {
		let mut drv_feats = FeatureSet::new(0);

		for feat in wanted_feats.iter() {
			drv_feats |= *feat;
		}

		let dev_feats = FeatureSet::new(self.com_cfg.dev_features());

		// Checks if the selected feature set is compatible with requirements for
		// features according to Virtio spec. v1.1 - 5.11.3.
		match FeatureSet::check_features(wanted_feats) {
			Ok(_) => {
				debug!("Feature set wanted by filesystem driver are in conformance with specification.")
			}
			Err(fs_err) => return Err(fs_err),
		}

		if (dev_feats & drv_feats) == drv_feats {
			// If device supports subset of features write feature set to common config
			self.com_cfg.set_drv_features(drv_feats.into());
			Ok(())
		} else {
			Err(VirtioVsockError::IncompFeatsSet(drv_feats, dev_feats))
		}
	}

	/// Initializes the device in adherence to specification. Returns Some(VirtioVsockError)
	/// upon failure and None in case everything worked as expected.
	///
	/// See Virtio specification v1.1. - 3.1.1.
	///                      and v1.1. - 5.10.6
	pub(crate) fn init_dev(&mut self) -> Result<(), VirtioVsockError> {
		// Reset
		self.com_cfg.reset_dev();

		// Indiacte device, that OS noticed it
		self.com_cfg.ack_dev();

		// Indicate device, that driver is able to handle it
		self.com_cfg.set_drv();

		let feats: Vec<Features> = vec![Features::VIRTIO_F_VERSION_1];
		self.negotiate_features(&feats)?;

		// Indicates the device, that the current feature set is final for the driver
		// and will not be changed.
		self.com_cfg.features_ok();

		// Checks if the device has accepted final set. This finishes feature negotiation.
		if self.com_cfg.check_features() {
			info!(
				"Features have been negotiated between virtio socket device {:x} and driver.",
				self.dev_cfg.dev_id
			);
			// Set feature set in device config fur future use.
			self.dev_cfg.features.set_features(&feats);
		} else {
			return Err(VirtioVsockError::FailFeatureNeg(self.dev_cfg.dev_id));
		}

		// create the queues and tell device about them
		for i in 0..3u16 {
			let vq = Virtq::new(
				&mut self.com_cfg,
				&self.notif_cfg,
				VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
				VqType::Split,
				VqIndex::from(i),
				self.dev_cfg.features.into(),
			);
			self.vqueues.push(Rc::new(vq));
		}

		Ok(())
	}
}

pub mod constants {
	use alloc::vec::Vec;
	use core::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign};

	pub use super::error::VirtioVsockError;

	/// Enum contains virtio's socket device features and general features of Virtio.
	///
	/// See Virtio specification v1.1. - 5.11.3
	///
	/// See Virtio specification v1.1. - 6
	//
	// WARN: In case the enum is changed, the static function of features `into_features(feat: u64) ->
	// Option<Vec<Features>>` must also be adjusted to return a correct vector of features.
	#[allow(dead_code, non_camel_case_types)]
	#[derive(Copy, Clone, Debug)]
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

	impl core::fmt::Display for Features {
		fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
			match *self {
				Features::VIRTIO_F_RING_INDIRECT_DESC => write!(f, "VIRTIO_F_RING_INDIRECT_DESC"),
				Features::VIRTIO_F_RING_EVENT_IDX => write!(f, "VIRTIO_F_RING_EVENT_IDX"),
				Features::VIRTIO_F_VERSION_1 => write!(f, "VIRTIO_F_VERSION_1"),
				Features::VIRTIO_F_ACCESS_PLATFORM => write!(f, "VIRTIO_F_ACCESS_PLATFORM"),
				Features::VIRTIO_F_RING_PACKED => write!(f, "VIRTIO_F_RING_PACKED"),
				Features::VIRTIO_F_IN_ORDER => write!(f, "VIRTIO_F_IN_ORDER"),
				Features::VIRTIO_F_ORDER_PLATFORM => write!(f, "VIRTIO_F_ORDER_PLATFORM"),
				Features::VIRTIO_F_SR_IOV => write!(f, "VIRTIO_F_SR_IOV"),
				Features::VIRTIO_F_NOTIFICATION_DATA => write!(f, "VIRTIO_F_NOTIFICATION_DATA"),
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

			if vec_of_feats.is_empty() {
				None
			} else {
				Some(vec_of_feats)
			}
		}
	}

	/// FeatureSet is new type whicih holds features for virito network devices indicated by the virtio specification
	/// v1.1. - 5.1.3. and all General Features defined in Virtio specification v1.1. - 6
	/// wrapping a u64.
	///
	/// The main functionality of this type are functions implemented on it.
	#[derive(Debug, Copy, Clone, PartialOrd, PartialEq, Eq)]
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
		/// specfification v1.1. - 5.11.3
		/// Upon an error returns the incompatible set of features by the
		/// [FeatReqNotMet](super::error::VirtioFsError) error value, which
		/// wraps the u64 indicating the feature set.
		///
		/// INFO: Iterates twice over the vector of features.
		pub fn check_features(feats: &[Features]) -> Result<(), VirtioVsockError> {
			let mut feat_bits = 0u64;

			for feat in feats.iter() {
				feat_bits |= *feat;
			}

			for feat in feats {
				match feat {
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
		/// WARN: Features should be checked before using this function via the [`FeatureSet::check_features`] function.
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

/// Error module of virtio socket device driver.
pub mod error {
	use super::constants::FeatureSet;

	/// Virtio socket device error enum.
	#[derive(Debug, Copy, Clone)]
	pub enum VirtioVsockError {
		NoDevCfg(u16),
		NoComCfg(u16),
		NoIsrCfg(u16),
		NoNotifCfg(u16),
		FailFeatureNeg(u16),
		/// The first u64 contains the feature bits wanted by the driver.
		/// but which are incompatible with the device feature set, second u64.
		IncompFeatsSet(FeatureSet, FeatureSet),
		Unknown,
	}
}
