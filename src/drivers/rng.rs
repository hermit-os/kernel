//! A virtio-rng driver.
//!
//! For details on the device, see [Entropy Device].
//!
//! [Entropy Device]: https://docs.oasis-open.org/virtio/virtio/v1.4/cs01/virtio-v1.4-cs01.html#x1-4130004
//!

use alloc::vec::Vec;

use embedded_io::{ErrorType, Read};
use pci_types::InterruptLine;
use smallvec::SmallVec;
use volatile::VolatileRef;
use volatile::access::ReadOnly;

use crate::config::{RNG_PACKET_SIZE, VIRTIO_MAX_QUEUE_SIZE};
use crate::drivers::error::DriverError;
#[cfg(not(feature = "pci"))]
use crate::drivers::mmio::get_rng_driver;
#[cfg(feature = "pci")]
use crate::drivers::pci::get_rng_driver;
use crate::drivers::virtio::error::VirtioRngError;
use crate::drivers::virtio::transport::UniCapsColl;
use crate::drivers::virtio::virtqueue::split::SplitVq;
use crate::drivers::virtio::virtqueue::{
	AvailBufferToken, BufferElem, BufferType, UsedBufferToken, VirtQueue, Virtq,
};
use crate::drivers::{Driver, InterruptHandlerMap};
use crate::errno::Errno;
use crate::mm::device_alloc::DeviceAlloc;

pub fn seed_entropy() -> Option<[u8; 32]> {
	get_rng_driver().and_then(|drv| {
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
				panic!("Setup of rng queue failed, which should not happen!");
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
			packet_size: RNG_PACKET_SIZE,
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
pub(crate) struct VirtioRngDriver {
	pub(super) recv_vq: RxQueue,
}

impl Driver for VirtioRngDriver {
	fn get_name() -> &'static str {
		"virtio-rng"
	}
}

impl ErrorType for VirtioRngDriver {
	type Error = Errno;
}

impl super::virtio::VirtioDriver for VirtioRngDriver {
	type Config = ();
	type Error = VirtioRngError;
	type DeviceFeatures = virtio::F;

	const MINIMAL_FEATURES: Self::DeviceFeatures = virtio::F::VERSION_1;
	const OPTIONAL_FEATURES: Self::DeviceFeatures = virtio::F::empty();

	fn init_dev(
		(mut caps_coll, dev_cfg_raw): (UniCapsColl, VolatileRef<'static, Self::Config, ReadOnly>),
		_handlers: &mut InterruptHandlerMap,
		_irq: Option<InterruptLine>,
	) -> Result<Self, (crate::drivers::virtio::error::VirtioError, UniCapsColl)> {
		let mut recv_vq = RxQueue::new();

		let _dev_cfg: crate::drivers::virtio::DevCfg<Self> =
			match caps_coll.init_caps(dev_cfg_raw, |caps_coll, dev_cfg| {
				recv_vq.add(VirtQueue::Split(
					SplitVq::new(
						&mut caps_coll.com_cfg,
						&caps_coll.notif_cfg,
						VIRTIO_MAX_QUEUE_SIZE,
						0,
						dev_cfg.features,
					)
					.unwrap(),
				));
				recv_vq.disable_notifs();

				Ok(())
			}) {
				Ok(dev_cfg) => dev_cfg,
				Err(err) => return Err((err, caps_coll)),
			};

		Ok(Self { recv_vq })
	}

	#[cfg(feature = "pci")]
	fn no_dev_cfg_err(dev_id: u16) -> Self::Error {
		VirtioRngError::NoDevCfg(dev_id)
	}
}

impl Read for VirtioRngDriver {
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
	pub enum VirtioRngError {
		#[cfg(feature = "pci")]
		#[error(
			"Virtio rng device driver failed, for device {0:x}, due to a missing or malformed device config!"
		)]
		NoDevCfg(u16),
	}
}
