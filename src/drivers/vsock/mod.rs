#[cfg(feature = "pci")]
pub mod pci;

use alloc::rc::Rc;
use alloc::vec::Vec;

use virtio::FeatureBits;

use crate::config::VIRTIO_MAX_QUEUE_SIZE;
use crate::drivers::virtio::error::VirtioVsockError;
#[cfg(feature = "pci")]
use crate::drivers::virtio::transport::pci::{ComCfg, IsrStatus, NotifCfg};
use crate::drivers::virtio::virtqueue::split::SplitVq;
use crate::drivers::virtio::virtqueue::{Virtq, VqIndex, VqSize};
#[cfg(feature = "pci")]
use crate::drivers::vsock::pci::VsockDevCfgRaw;

/// A wrapper struct for the raw configuration structure.
/// Handling the right access to fields, as some are read-only
/// for the driver.
pub(crate) struct VsockDevCfg {
	pub raw: &'static VsockDevCfgRaw,
	pub dev_id: u16,
	pub features: virtio::net::F,
}

pub(crate) struct VirtioVsockDriver {
	pub(super) dev_cfg: VsockDevCfg,
	pub(super) com_cfg: ComCfg,
	pub(super) isr_stat: IsrStatus,
	pub(super) notif_cfg: NotifCfg,
	pub(super) vqueues: Vec<Rc<dyn Virtq>>,
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

	pub fn disable_interrupts(&self) {
		// For send and receive queues?
		// Only for receive? Because send is off anyway?
		self.vqueues[0].disable_notifs();
	}

	pub fn enable_interrupts(&self) {
		// For send and receive queues?
		// Only for receive? Because send is off anyway?
		self.vqueues[0].enable_notifs();
	}

	/// Negotiates a subset of features, understood and wanted by both the OS
	/// and the device.
	fn negotiate_features(
		&mut self,
		driver_features: virtio::vsock::F,
	) -> Result<(), VirtioVsockError> {
		let device_features = virtio::vsock::F::from(self.com_cfg.dev_features());

		if device_features.requirements_satisfied() {
			info!("Feature set wanted by vsock driver are in conformance with specification.");
		} else {
			return Err(VirtioVsockError::FeatureRequirementsNotMet(device_features));
		}

		if device_features.contains(driver_features) {
			// If device supports subset of features write feature set to common config
			self.com_cfg.set_drv_features(driver_features.into());
			Ok(())
		} else {
			Err(VirtioVsockError::IncompatibleFeatureSets(
				driver_features,
				device_features,
			))
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

		let features = virtio::vsock::F::VERSION_1;
		self.negotiate_features(features)?;

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
			self.dev_cfg.features = features;
		} else {
			return Err(VirtioVsockError::FailFeatureNeg(self.dev_cfg.dev_id));
		}

		// create the queues and tell device about them
		for i in 0..3u16 {
			let vq = SplitVq::new(
				&mut self.com_cfg,
				&self.notif_cfg,
				VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
				VqIndex::from(i),
				self.dev_cfg.features.into(),
			)
			.unwrap();
			self.vqueues.push(Rc::new(vq));
		}

		Ok(())
	}
}

/// Error module of virtio socket device driver.
pub mod error {
	/// Virtio socket device error enum.
	#[derive(Debug, Copy, Clone)]
	pub enum VirtioVsockError {
		NoDevCfg(u16),
		NoComCfg(u16),
		NoIsrCfg(u16),
		NoNotifCfg(u16),
		FailFeatureNeg(u16),
		/// Set of features does not adhere to the requirements of features
		/// indicated by the specification
		FeatureRequirementsNotMet(virtio::net::F),
		/// The first u64 contains the feature bits wanted by the driver.
		/// but which are incompatible with the device feature set, second u64.
		IncompatibleFeatureSets(virtio::net::F, virtio::net::F),
	}
}
