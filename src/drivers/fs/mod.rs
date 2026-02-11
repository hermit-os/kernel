//! A virtio-fs driver.
//!
//! For details on the device, see [File System Device].
//! For details on the Rust definitions, see [`virtio::fs`].
//!
//! [File System Device]: https://docs.oasis-open.org/virtio/virtio/v1.2/cs01/virtio-v1.2-cs01.html#x1-45800011

cfg_if::cfg_if! {
	if #[cfg(feature = "pci")] {
		mod pci;
	} else {
		mod mmio;
	}
}

use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::mem::MaybeUninit;
use core::{mem, str};

use fuse_abi::linux::fuse_out_header;
use num_enum::TryFromPrimitive;
use pci_types::InterruptLine;
use smallvec::SmallVec;
use virtio::fs::ConfigVolatileFieldAccess;
use volatile::VolatileRef;
use volatile::access::ReadOnly;

use crate::config::VIRTIO_MAX_QUEUE_SIZE;
use crate::drivers::Driver;
use crate::drivers::virtio::ControlRegisters;
use crate::drivers::virtio::error::VirtioFsInitError;
#[cfg(not(feature = "pci"))]
use crate::drivers::virtio::transport::mmio::{ComCfg, IsrStatus, NotifCfg};
#[cfg(feature = "pci")]
use crate::drivers::virtio::transport::pci::{ComCfg, IsrStatus, NotifCfg};
use crate::drivers::virtio::virtqueue::error::VirtqError;
use crate::drivers::virtio::virtqueue::split::SplitVq;
use crate::drivers::virtio::virtqueue::{
	AvailBufferToken, BufferElem, BufferType, VirtQueue, Virtq,
};
use crate::errno::Errno;
use crate::fs::virtio_fs::{self, Rsp, RspHeader, VirtioFsError, VirtioFsInterface};
use crate::mm::device_alloc::DeviceAlloc;

/// A wrapper struct for the raw configuration structure.
/// Handling the right access to fields, as some are read-only
/// for the driver.
pub(crate) struct FsDevCfg {
	pub raw: VolatileRef<'static, virtio::fs::Config, ReadOnly>,
	pub dev_id: u16,
	pub features: virtio::fs::F,
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
	pub(super) vqueues: Vec<VirtQueue>,
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

	/// Initializes the device in adherence to specification. Returns Some(VirtioFsError)
	/// upon failure and None in case everything worked as expected.
	///
	/// See Virtio specification v1.1. - 3.1.1.
	///                      and v1.1. - 5.11.5
	pub(crate) fn init_dev(&mut self) -> Result<(), VirtioFsInitError> {
		// Reset
		self.com_cfg.reset_dev();

		// Indicate device, that OS noticed it
		self.com_cfg.ack_dev();

		// Indicate device, that driver is able to handle it
		self.com_cfg.set_drv();

		let minimal_features = virtio::fs::F::VERSION_1;
		let negotiated_features = self
			.com_cfg
			.control_registers()
			.negotiate_features(minimal_features);

		if !negotiated_features.contains(minimal_features) {
			error!("Device features set, does not satisfy minimal features needed. Aborting!");
			return Err(VirtioFsInitError::FailFeatureNeg(self.dev_cfg.dev_id));
		}

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
			self.dev_cfg.features = negotiated_features;
		} else {
			error!("The device does not support our subset of features.");
			return Err(VirtioFsInitError::FailFeatureNeg(self.dev_cfg.dev_id));
		}

		// 1 highprio queue, and n normal request queues
		let vqnum = self
			.dev_cfg
			.raw
			.as_ptr()
			.num_request_queues()
			.read()
			.to_ne() + 1;
		if vqnum == 0 {
			error!("0 request queues requested from device. Aborting!");
			return Err(VirtioFsInitError::Unknown);
		}

		// create the queues and tell device about them
		for i in 0..vqnum as u16 {
			let vq = VirtQueue::Split(
				SplitVq::new(
					&mut self.com_cfg,
					&self.notif_cfg,
					VIRTIO_MAX_QUEUE_SIZE,
					i,
					self.dev_cfg.features.into(),
				)
				.unwrap(),
			);
			self.vqueues.push(vq);
		}

		// At this point the device is "live"
		self.com_cfg.drv_ok();

		Ok(())
	}

	pub fn handle_interrupt(&mut self) {
		let status = self.isr_stat.acknowledge();

		#[cfg(not(feature = "pci"))]
		if status.contains(virtio::mmio::InterruptStatus::CONFIGURATION_CHANGE_NOTIFICATION) {
			info!("Configuration changes are not possible! Aborting");
			todo!("Implement possibility to change config on the fly...")
		}

		#[cfg(feature = "pci")]
		if status.contains(virtio::pci::IsrStatus::DEVICE_CONFIGURATION_INTERRUPT) {
			info!("Configuration changes are not possible! Aborting");
			todo!("Implement possibility to change config on the fly...")
		}
	}
}

impl VirtioFsInterface for VirtioFsDriver {
	fn send_command<O: virtio_fs::ops::Op + 'static>(
		&mut self,
		cmd: virtio_fs::Cmd<O>,
		rsp_payload_len: u32,
	) -> Result<virtio_fs::Rsp<O>, VirtioFsError>
	where
		<O as virtio_fs::ops::Op>::InStruct: Send,
		<O as virtio_fs::ops::Op>::OutStruct: Send,
	{
		let virtio_fs::Cmd {
			headers: cmd_headers,
			payload: cmd_payload_opt,
		} = cmd;
		let send = if let Some(cmd_payload) = cmd_payload_opt {
			SmallVec::from_buf([
				BufferElem::Sized(cmd_headers),
				BufferElem::Vector(cmd_payload),
			])
		} else {
			let mut vec = SmallVec::new();
			vec.push(BufferElem::Sized(cmd_headers));
			vec
		};

		// If the operation fails, it is possible for its header to be uninitialized.
		// For this reason, we use a instantiation of the RspHeader structure where
		// the op_header field is MaybeUninit
		let rsp_headers =
			Box::<RspHeader<O, MaybeUninit<O::OutStruct>>, _>::new_uninit_in(DeviceAlloc);
		let recv = if rsp_payload_len == 0 {
			let mut vec = SmallVec::new();
			vec.push(BufferElem::Sized(rsp_headers));
			vec
		} else {
			SmallVec::from_buf([
				BufferElem::Sized(rsp_headers),
				BufferElem::Vector(Vec::with_capacity_in(rsp_payload_len as usize, DeviceAlloc)),
			])
		};

		let buffer_tkn = AvailBufferToken::new(send, recv).unwrap();
		let mut transfer_result =
			self.vqueues[1].dispatch_blocking(buffer_tkn, BufferType::Direct)?;

		let (dyn_headers, written_header_len) =
			transfer_result.used_recv_buff.pop_front_raw().unwrap();
		let headers = dyn_headers
			.downcast::<MaybeUninit<RspHeader<O, MaybeUninit<O::OutStruct>>>>()
			.unwrap();
		if written_header_len < size_of::<fuse_out_header>() {
			return Err(VirtqError::IncompleteWrite.into());
		}

		// SAFETY: we confirmed that the out_header was written. The op_header does not need to be initialized at this stage,
		// as it is behind a nested MaybeUninit.
		let headers = unsafe { headers.assume_init() };

		if headers.out_header.error != 0
			|| (written_header_len - size_of::<fuse_out_header>()) != size_of::<O::OutStruct>()
		{
			// "However, if the reply is an error reply (i.e., error is set), then no further payload data should be sent,
			// independent of the request." (fuse man page)

			return Err(VirtioFsError::IOError(
				Errno::try_from_primitive(-headers.out_header.error).unwrap_or(Errno::Io),
			));
		}

		// SAFETY: the conditional above ensures that the second field was filled in, so we can transmute it from MaybeUninit to normal.
		let headers = unsafe {
			mem::transmute::<Box<RspHeader<O, MaybeUninit<O::OutStruct>>, _>, Box<RspHeader<O>, _>>(
				headers,
			)
		};
		let payload = transfer_result.used_recv_buff.pop_front_vec();
		Ok(Rsp { headers, payload })
	}

	fn get_mount_point(&self) -> String {
		let tag = self.dev_cfg.raw.as_ptr().tag().read();
		let tag = str::from_utf8(&tag).unwrap();
		let tag = tag.split('\0').next().unwrap();
		tag.to_owned()
	}
}

impl Driver for VirtioFsDriver {
	fn get_interrupt_number(&self) -> InterruptLine {
		self.irq
	}

	fn get_name(&self) -> &'static str {
		"virtio"
	}
}

/// Error module of virtios filesystem driver.
pub mod error {
	use thiserror::Error;

	/// Network filesystem error enum.
	#[derive(Error, Debug, Copy, Clone)]
	pub enum VirtioFsInitError {
		#[cfg(feature = "pci")]
		#[error(
			"Virtio filesystem driver failed, for device {0:x}, due to a missing or malformed device config!"
		)]
		NoDevCfg(u16),

		#[error(
			"Virtio filesystem driver failed, for device {0:x}, device did not acknowledge negotiated feature set!"
		)]
		FailFeatureNeg(u16),

		#[error("Virtio filesystem failed, driver failed due unknown reason!")]
		Unknown,
	}
}
