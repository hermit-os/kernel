use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use pci_types::InterruptLine;
use virtio_spec::FeatureBits;

use crate::config::VIRTIO_MAX_QUEUE_SIZE;
#[cfg(feature = "pci")]
use crate::drivers::fs::virtio_pci::FsDevCfgRaw;
use crate::drivers::virtio::error::VirtioFsError;
#[cfg(not(feature = "pci"))]
use crate::drivers::virtio::transport::mmio::{ComCfg, IsrStatus, NotifCfg};
#[cfg(feature = "pci")]
use crate::drivers::virtio::transport::pci::{ComCfg, IsrStatus, NotifCfg};
use crate::drivers::virtio::virtqueue::error::VirtqError;
use crate::drivers::virtio::virtqueue::split::SplitVq;
use crate::drivers::virtio::virtqueue::{AsSliceU8, BufferType, Virtq, VqIndex, VqSize};
use crate::fs::fuse::{self, FuseInterface};

/// A wrapper struct for the raw configuration structure.
/// Handling the right access to fields, as some are read-only
/// for the driver.
pub(crate) struct FsDevCfg {
	pub raw: &'static FsDevCfgRaw,
	pub dev_id: u16,
	pub features: virtio_spec::fs::F,
}

/// Virtio file system driver struct.
///
/// Struct allows to control devices virtqueues as also
/// the device itself.
#[allow(dead_code)]
pub(crate) struct VirtioFsDriver {
	pub(super) dev_cfg: FsDevCfg,
	pub(super) com_cfg: ComCfg,
	pub(super) isr_stat: IsrStatus,
	pub(super) notif_cfg: NotifCfg,
	pub(super) vqueues: Vec<Rc<dyn Virtq>>,
	pub(super) irq: InterruptLine,
}

// Backend-independent interface for Virtio network driver
impl VirtioFsDriver {
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
	fn negotiate_features(
		&mut self,
		driver_features: virtio_spec::fs::F,
	) -> Result<(), VirtioFsError> {
		let device_features = virtio_spec::fs::F::from(self.com_cfg.dev_features());

		if device_features.requirements_satisfied() {
			debug!("Feature set wanted by filesystem driver are in conformance with specification.")
		} else {
			return Err(VirtioFsError::FeatureRequirementsNotMet(device_features));
		}

		if device_features.contains(driver_features) {
			// If device supports subset of features write feature set to common config
			self.com_cfg.set_drv_features(driver_features.into());
			Ok(())
		} else {
			Err(VirtioFsError::IncompatibleFeatureSets(
				driver_features,
				device_features,
			))
		}
	}

	/// Initializes the device in adherence to specification. Returns Some(VirtioFsError)
	/// upon failure and None in case everything worked as expected.
	///
	/// See Virtio specification v1.1. - 3.1.1.
	///                      and v1.1. - 5.11.5
	pub(crate) fn init_dev(&mut self) -> Result<(), VirtioFsError> {
		// Reset
		self.com_cfg.reset_dev();

		// Indiacte device, that OS noticed it
		self.com_cfg.ack_dev();

		// Indicate device, that driver is able to handle it
		self.com_cfg.set_drv();

		let features = virtio_spec::fs::F::VERSION_1;
		self.negotiate_features(features)?;

		// Indicates the device, that the current feature set is final for the driver
		// and will not be changed.
		self.com_cfg.features_ok();

		// Checks if the device has accepted final set. This finishes feature negotiation.
		if self.com_cfg.check_features() {
			info!(
				"Features have been negotiated between virtio filesystem device {:x} and driver.",
				self.dev_cfg.dev_id
			);
			// Set feature set in device config fur future use.
			self.dev_cfg.features = features;
		} else {
			return Err(VirtioFsError::FailFeatureNeg(self.dev_cfg.dev_id));
		}

		// 1 highprio queue, and n normal request queues
		let vqnum = self.dev_cfg.raw.get_num_queues() + 1;
		if vqnum == 0 {
			error!("0 request queues requested from device. Aborting!");
			return Err(VirtioFsError::Unknown);
		}

		// create the queues and tell device about them
		for i in 0..vqnum as u16 {
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

		// At this point the device is "live"
		self.com_cfg.drv_ok();

		Ok(())
	}
}

impl FuseInterface for VirtioFsDriver {
	fn send_command<O: fuse::ops::Op>(
		&mut self,
		cmd: (Box<fuse::CmdHeader<O>>, Option<Box<[u8]>>),
		rsp: &mut fuse::Rsp<O>,
	) -> Result<(), VirtqError> {
		let (cmd_header, cmd_payload_opt) = cmd;
		let send: &[&[u8]] = if let Some(cmd_payload) = cmd_payload_opt.as_deref() {
			&[cmd_header.as_slice_u8(), cmd_payload]
		} else {
			&[cmd_header.as_slice_u8()]
		};
		let recv = &[rsp.as_slice_u8_mut()];
		let transfer_tkn = self.vqueues[1]
			.clone()
			.prep_transfer_from_raw(send, recv, BufferType::Direct)
			.unwrap();
		transfer_tkn.dispatch_blocking()?;
		Ok(())
	}

	fn get_mount_point(&self) -> String {
		self.dev_cfg.raw.get_tag().to_string()
	}
}

/// Error module of virtios filesystem driver.
pub mod error {
	/// Network filesystem error enum.
	#[derive(Debug, Copy, Clone)]
	pub enum VirtioFsError {
		#[cfg(feature = "pci")]
		NoDevCfg(u16),
		#[cfg(feature = "pci")]
		NoComCfg(u16),
		#[cfg(feature = "pci")]
		NoIsrCfg(u16),
		#[cfg(feature = "pci")]
		NoNotifCfg(u16),
		FailFeatureNeg(u16),
		/// The first field contains the feature bits wanted by the driver.
		/// but which are incompatible with the device feature set, second field.
		IncompatibleFeatureSets(virtio_spec::fs::F, virtio_spec::fs::F),
		/// Set of features does not adhere to the requirements of features
		/// indicated by the specification
		FeatureRequirementsNotMet(virtio_spec::fs::F),
		Unknown,
	}
}
