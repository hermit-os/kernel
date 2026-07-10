//! A virtio-entropy driver.
//!
//! For details on the device, see [Entropy Device].
//! For details on the Rust definitions, see [`virtio::entropy`].
//!
//! [Entropy Device]: https://docs.oasis-open.org/virtio/virtio/v1.4/virtio-v1.4.pdf#page=180.53
//!

#[cfg(not(feature = "pci"))]
mod mmio;
#[cfg(feature = "pci")]
mod pci;

use alloc::vec::Vec;

use embedded_io::{ErrorType, Read};
use smallvec::SmallVec;

use crate::config::{ENTROPY_PACKET_SIZE, VIRTIO_MAX_QUEUE_SIZE};
use crate::drivers::error::DriverError;
#[cfg(not(feature = "pci"))]
use crate::drivers::mmio::get_entropy_driver;
#[cfg(feature = "pci")]
use crate::drivers::pci::get_entropy_driver;
use crate::drivers::virtio::ControlRegisters;
use crate::drivers::virtio::error::VirtioEntropyError;
#[cfg(not(feature = "pci"))]
use crate::drivers::virtio::transport::mmio::{ComCfg, NotifCfg};
#[cfg(feature = "pci")]
use crate::drivers::virtio::transport::pci::{ComCfg, NotifCfg};
use crate::drivers::virtio::virtqueue::split::SplitVq;
use crate::drivers::virtio::virtqueue::{
	AvailBufferToken, BufferElem, BufferType, UsedBufferToken, VirtQueue, Virtq,
};
use crate::errno::Errno;
use crate::mm::device_alloc::DeviceAlloc;

pub fn seed_entropy() -> Option<[u8; 32]> {
	get_entropy_driver().and_then(|drv| {
		let mut buf = [0u8; 32];
		if drv.lock().read(&mut buf).ok()? == buf.len() {
			Some(buf)
		} else {
			None
		}
	})
}

fn fill_queue(vq: &mut VirtQueue, num_packets: u16, packet_size: u32) {
	for _ in 0..num_packets {
		let buff_tkn = match AvailBufferToken::new(SmallVec::new(), {
			let mut vec = SmallVec::new();
			vec.push(BufferElem::Vector(Vec::with_capacity_in(
				packet_size.try_into().unwrap(),
				DeviceAlloc,
			)));
			vec
		}) {
			Ok(tkn) => tkn,
			Err(_vq_err) => {
				panic!("Setup of entropy queue failed, which should not happen!");
			}
		};

		// BufferTokens are directly provided to the queue
		// TransferTokens are directly dispatched
		// Transfers will be awaited at the queue
		if let Err(err) = vq.dispatch(buff_tkn, false, BufferType::Direct) {
			error!("{err:#?}");
			break;
		}
	}
}

pub(crate) struct RxQueue {
	vq: Option<VirtQueue>,
	packet_size: u32,
}
impl RxQueue {
	pub fn new() -> Self {
		Self {
			vq: None,
			packet_size: ENTROPY_PACKET_SIZE,
		}
	}

	pub fn add(&mut self, mut vq: VirtQueue) {
		const BUFF_PER_PACKET: u16 = 1;
		let num_packets = vq.size() / BUFF_PER_PACKET;
		fill_queue(&mut vq, num_packets, self.packet_size);

		self.vq = Some(vq);
	}

	pub fn disable_notifs(&mut self) {
		let Some(vq) = &mut self.vq else {
			return;
		};

		vq.disable_notifs();
	}

	fn get_next(&mut self) -> Option<UsedBufferToken> {
		self.vq.as_mut().unwrap().try_recv().ok()
	}

	pub fn process_packet<F>(&mut self, mut f: F) -> Result<usize, DriverError>
	where
		F: FnMut(&[u8]) -> usize,
	{
		let Some(mut buffer_tkn) = self.get_next() else {
			return Ok(0);
		};

		let packet = buffer_tkn.used_recv_buff.pop_front_vec().unwrap();
		let vq = self.vq.as_mut().unwrap();
		let result = f(&packet[..]);

		fill_queue(vq, 1, self.packet_size);

		Ok(result)
	}
}

/// A wrapper struct for the raw configuration structure.
/// Handling the right access to fields, as some are read-only
/// for the driver.
pub(crate) struct EntropyDevCfg {
	pub dev_id: u16,
	pub features: virtio::entropy::F,
}

pub(crate) struct VirtioEntropyDriver {
	pub(super) dev_cfg: EntropyDevCfg,
	pub(super) com_cfg: ComCfg,
	pub(super) notif_cfg: NotifCfg,

	pub(super) recv_vq: RxQueue,
}

impl VirtioEntropyDriver {
	#[cfg(feature = "pci")]
	pub fn set_failed(&mut self) {
		self.com_cfg.set_failed();
	}

	pub fn init_dev(&mut self) -> Result<(), VirtioEntropyError> {
		// Reset
		self.com_cfg.reset_dev();

		// Indicate device, that OS noticed it
		self.com_cfg.ack_dev();

		// Indicate device, that driver is able to handle it
		self.com_cfg.set_drv();

		let minimal_features = virtio::entropy::F::VERSION_1;
		let negotiated_features = self
			.com_cfg
			.control_registers()
			.negotiate_features(minimal_features);

		if !negotiated_features.contains(minimal_features) {
			error!("Device features set, does not satisfy minimal features needed. Aborting!");
			return Err(VirtioEntropyError::FailFeatureNeg(self.dev_cfg.dev_id));
		}

		// Indicates the device, that the current feature set is final for the driver
		// and will not be changed.
		self.com_cfg.features_ok();

		// Checks if the device has accepted final set. This finishes feature negotiation.
		if self.com_cfg.check_features() {
			info!(
				"Features have been negotiated between virtio entropy device {:x} and driver.",
				self.dev_cfg.dev_id
			);
			// Set feature set in device config fur future use.
			self.dev_cfg.features = negotiated_features;
		} else {
			error!("The device does not support our subset of features.");
			return Err(VirtioEntropyError::FailFeatureNeg(self.dev_cfg.dev_id));
		}

		// create the queues and tell device about them
		self.recv_vq.add(VirtQueue::Split(
			SplitVq::new(
				&mut self.com_cfg,
				&self.notif_cfg,
				VIRTIO_MAX_QUEUE_SIZE,
				0,
				self.dev_cfg.features.into(),
			)
			.unwrap(),
		));
		// Interrupt for receiving packets is not needed
		self.recv_vq.disable_notifs();

		// At this point the device is "live"
		self.com_cfg.drv_ok();

		Ok(())
	}
}

impl ErrorType for VirtioEntropyDriver {
	type Error = Errno;
}

impl Read for VirtioEntropyDriver {
	fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
		self.recv_vq
			.process_packet(|src| {
				buf[..src.len()].copy_from_slice(src);
				src.len()
			})
			.map_err(|_| Errno::Io)
	}
}

pub mod error {
	use thiserror::Error;

	#[derive(Error, Debug, Copy, Clone)]
	pub enum VirtioEntropyError {
		#[cfg(feature = "pci")]
		#[error(
			"Virtio entropy device driver failed, for device {0:x}, due to a missing or malformed device config!"
		)]
		NoDevCfg(u16),

		#[error(
			"Virtio entropy device driver failed, for device {0:x}, device did not acknowledge negotiated feature set!"
		)]
		FailFeatureNeg(u16),
	}
}
