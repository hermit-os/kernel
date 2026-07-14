//! A virtio-fs driver.
//!
//! For details on the device, see [File System Device].
//! For details on the Rust definitions, see [`virtio::fs`].
//!
//! [File System Device]: https://docs.oasis-open.org/virtio/virtio/v1.2/cs01/virtio-v1.2-cs01.html#x1-45800011

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
#[cfg(not(feature = "pci"))]
use crate::drivers::mmio::get_filesystem_driver;
#[cfg(feature = "pci")]
use crate::drivers::pci::get_filesystem_driver;
use crate::drivers::virtio::error::{VirtioError, VirtioFsInitError};
use crate::drivers::virtio::transport::{InterruptCapability, UniCapsColl};
use crate::drivers::virtio::virtqueue::error::VirtqError;
use crate::drivers::virtio::virtqueue::split::SplitVq;
use crate::drivers::virtio::virtqueue::{
	AvailBufferToken, BufferElem, BufferType, VirtQueue, Virtq,
};
use crate::drivers::{Driver, InterruptHandlerMap};
use crate::errno::Errno;
use crate::fs::virtio_fs::{self, Rsp, RspHeader, VirtioFsError, VirtioFsInterface};
use crate::mm::device_alloc::DeviceAlloc;

type FsDevCfg = super::virtio::DevCfg<VirtioFsDriver>;

/// Virtio file system driver struct.
///
/// Struct allows to control devices virtqueues as also
/// the device itself.
#[allow(dead_code)]
pub(crate) struct VirtioFsDriver {
	pub(super) dev_cfg: FsDevCfg,
	pub(super) caps_coll: UniCapsColl,
	pub(super) vqueues: Vec<VirtQueue>,
}

// Backend-independent interface for Virtio network driver
impl super::virtio::VirtioDriver for VirtioFsDriver {
	type Config = virtio::fs::Config;
	type Error = VirtioFsInitError;
	type DeviceFeatures = virtio::fs::F;

	const MINIMAL_FEATURES: Self::DeviceFeatures = virtio::fs::F::VERSION_1;
	const OPTIONAL_FEATURES: Self::DeviceFeatures = virtio::fs::F::empty();

	/// Initializes the device in adherence to specification. Returns Some(VirtioFsError)
	/// upon failure and None in case everything worked as expected.
	///
	/// See Virtio specification v1.1. - 3.1.1.
	///                      and v1.1. - 5.11.5
	fn init_dev(
		(mut caps_coll, dev_cfg_raw): (
			UniCapsColl,
			VolatileRef<'static, virtio::fs::Config, ReadOnly>,
		),
		handlers: &mut InterruptHandlerMap,
		irq: Option<InterruptLine>,
	) -> Result<Self, (VirtioError, UniCapsColl)> {
		let mut vqueues = Vec::new();

		let dev_cfg = match caps_coll.init_caps(dev_cfg_raw, |caps_coll, dev_cfg| {
			// 1 highprio queue, and n normal request queues
			let vqnum = dev_cfg.raw.as_ptr().num_request_queues().read().to_ne() + 1;
			if vqnum == 0 {
				error!("0 request queues requested from device. Aborting!");
				return Err(VirtioFsInitError::Unknown);
			}

			// create the queues and tell device about them
			for i in 0..vqnum as u16 {
				let vq = VirtQueue::Split(
					SplitVq::new(
						&mut caps_coll.com_cfg,
						&caps_coll.notif_cfg,
						VIRTIO_MAX_QUEUE_SIZE,
						i,
						virtio::F::from(dev_cfg.features),
					)
					.unwrap(),
				);
				vqueues.push(vq);
			}

			match &mut caps_coll.int_cap {
				InterruptCapability::IsrStatus(_) => {
					let irq = irq.unwrap();
					handlers.entry(irq).or_default().push_back(|| {
						if let Some(driver) = get_filesystem_driver() {
							driver.lock().handle_interrupt();
						};
					});
					crate::arch::kernel::interrupts::add_irq_name(irq, "virtio");
					info!("Virtio interrupt handler at line {irq}");
				}
				#[cfg(all(feature = "pci", target_arch = "x86_64"))]
				InterruptCapability::Msix(msix_table) => {
					use core::iter;

					caps_coll.com_cfg.register_msix_vectors(
						msix_table,
						handlers,
						|| {
							if let Some(driver) = get_filesystem_driver() {
								driver.lock().handle_device_configuration_interrupt();
							};
						},
						iter::empty::<(iter::Empty<_>, _)>(),
						0..vqnum as u16,
					);
				}
			}
			Ok(())
		}) {
			Ok(dev_cfg) => dev_cfg,
			Err(err) => return Err((err, caps_coll)),
		};

		Ok(Self {
			dev_cfg,
			caps_coll,
			vqueues,
		})
	}

	#[cfg(feature = "pci")]
	fn no_dev_cfg_err(dev_id: u16) -> Self::Error {
		VirtioFsInitError::NoDevCfg(dev_id)
	}
}

impl VirtioFsDriver {
	pub fn handle_interrupt(&mut self) {
		#[cfg_attr(
			not(all(feature = "pci", target_arch = "x86_64")),
			expect(irrefutable_let_patterns)
		)]
		let InterruptCapability::IsrStatus(ref mut isr_stat) = self.caps_coll.int_cap else {
			panic!("MSI-X vectors should be configured to the interrupt type-specific handlers.")
		};

		let status = isr_stat.acknowledge();

		let config_change = cfg_select! {
			feature = "pci" => virtio::pci::IsrStatus::DEVICE_CONFIGURATION_INTERRUPT,
			_ => virtio::mmio::InterruptStatus::CONFIGURATION_CHANGE_NOTIFICATION,
		};
		if status.contains(config_change) {
			self.handle_device_configuration_interrupt();
		}
	}

	fn handle_device_configuration_interrupt(&mut self) {
		if self.caps_coll.com_cfg.does_device_need_reset() {
			todo!("Device configuration change notification cannot be handled yet");
		}
	}
}

impl VirtioFsInterface for VirtioFsDriver {
	fn send_command<O: virtio_fs::ops::Op + 'static>(
		&mut self,
		cmd: virtio_fs::Cmd<O>,
		rsp_payload_len: u32,
	) -> Result<Rsp<O>, VirtioFsError>
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
			// independent of the request." (FUSE man page)

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
	fn get_name() -> &'static str {
		"virtio-fs"
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

		#[error("Virtio filesystem failed, driver failed due unknown reason!")]
		Unknown,
	}
}
