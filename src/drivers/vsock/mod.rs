#![allow(dead_code)]

#[cfg(feature = "pci")]
pub mod pci;

use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cmp::Ordering;
use core::mem;

use align_address::Align;
use pci_types::InterruptLine;
use virtio::vsock::{Event, Hdr};
use virtio::FeatureBits;

use crate::config::VIRTIO_MAX_QUEUE_SIZE;
use crate::drivers::virtio::error::VirtioVsockError;
#[cfg(feature = "pci")]
use crate::drivers::virtio::transport::pci::{ComCfg, IsrStatus, NotifCfg};
use crate::drivers::virtio::virtqueue::split::SplitVq;
use crate::drivers::virtio::virtqueue::{
	BuffSpec, BufferToken, BufferType, Bytes, Virtq, VqIndex, VqSize,
};
#[cfg(feature = "pci")]
use crate::drivers::vsock::pci::VsockDevCfgRaw;

const MTU: usize = 65536;

pub(crate) struct RxQueue {
	vq: Option<Rc<dyn Virtq>>,
	poll_sender: async_channel::Sender<BufferToken>,
	poll_receiver: async_channel::Receiver<BufferToken>,
}

impl RxQueue {
	pub fn new() -> Self {
		let (poll_sender, poll_receiver) = async_channel::unbounded();

		Self {
			vq: None,
			poll_sender,
			poll_receiver,
		}
	}

	pub fn add(&mut self, vq: Rc<dyn Virtq>) {
		let num_buff: u16 = vq.size().into();
		let rx_size = (MTU + mem::size_of::<Hdr>())
			.align_up(core::mem::size_of::<crossbeam_utils::CachePadded<u8>>());
		let spec = BuffSpec::Single(Bytes::new(rx_size).unwrap());

		for _ in 0..num_buff {
			let buff_tkn = match BufferToken::new(None, Some(spec.clone())) {
				Ok(tkn) => tkn,
				Err(_vq_err) => {
					error!("Setup of vsock queue failed, which should not happen!");
					panic!("setup of vsock queue failed!");
				}
			};

			// BufferTokens are directly provided to the queue
			// TransferTokens are directly dispatched
			// Transfers will be awaited at the queue
			match vq.dispatch_await(
				buff_tkn,
				self.poll_sender.clone(),
				false,
				BufferType::Direct,
			) {
				Ok(_) => (),
				Err(_) => {
					error!("Descriptor IDs were exhausted earlier than expected.");
					break;
				}
			}
		}

		self.vq = Some(vq);
	}

	pub fn enable_notifs(&self) {
		if let Some(ref vq) = self.vq {
			vq.enable_notifs();
		}
	}

	pub fn disable_notifs(&self) {
		if let Some(ref vq) = self.vq {
			vq.disable_notifs();
		}
	}

	fn get_next(&mut self) -> Option<BufferToken> {
		let transfer = self.poll_receiver.try_recv();

		transfer
			.or_else(|_| {
				// Check if any not yet provided transfers are in the queue.
				self.poll();

				self.poll_receiver.try_recv()
			})
			.ok()
	}

	fn poll(&self) {
		if let Some(ref vq) = self.vq {
			vq.poll();
		}
	}

	pub fn process_packet<F>(&mut self, mut f: F)
	where
		F: FnMut(&Hdr, &[u8]),
	{
		const HEADER_SIZE: usize = mem::size_of::<Hdr>();

		while let Some(mut buffer_tkn) = self.get_next() {
			let (_, recv_data_opt) = buffer_tkn.as_slices().unwrap();
			let mut recv_data = recv_data_opt.unwrap();

			if recv_data.len() == 1 {
				let packet = recv_data.pop().unwrap();

				// drop packets with invalid packet size
				if packet.len() < HEADER_SIZE {
					panic!("Invalid packet size!");
				}

				if let Some(ref vq) = self.vq {
					let header = unsafe {
						core::mem::transmute::<[u8; HEADER_SIZE], Hdr>(
							packet[..HEADER_SIZE].try_into().unwrap(),
						)
					};

					f(&header, &packet[HEADER_SIZE..]);

					buffer_tkn.reset();
					vq.dispatch_await(
						buffer_tkn,
						self.poll_sender.clone(),
						false,
						BufferType::Direct,
					)
					.unwrap();
				} else {
					panic!("Unable to get receive queue");
				}
			} else {
				panic!("Invalid length of receive queue");
			}
		}
	}
}

pub(crate) struct TxQueue {
	vq: Option<Rc<dyn Virtq>>,
	poll_sender: async_channel::Sender<BufferToken>,
	poll_receiver: async_channel::Receiver<BufferToken>,
	ready_queue: Vec<BufferToken>,
}

impl TxQueue {
	pub fn new() -> Self {
		let (poll_sender, poll_receiver) = async_channel::unbounded();

		Self {
			vq: None,
			poll_sender,
			poll_receiver,
			ready_queue: Vec::new(),
		}
	}

	pub fn add(&mut self, vq: Rc<dyn Virtq>) {
		let tx_size = (1514 + mem::size_of::<Hdr>())
			.align_up(core::mem::size_of::<crossbeam_utils::CachePadded<u8>>());
		let buff_def = Bytes::new(tx_size).unwrap();
		let spec = BuffSpec::Single(buff_def);
		let num_buff: u16 = vq.size().into();

		for _ in 0..num_buff {
			let mut buffer_tkn = BufferToken::new(Some(spec.clone()), None).unwrap();
			buffer_tkn
				.write_seq(Some(&Hdr::default()), None::<&Hdr>)
				.unwrap();
			self.ready_queue.push(buffer_tkn)
		}

		self.vq = Some(vq);
	}

	pub fn enable_notifs(&self) {
		if let Some(ref vq) = self.vq {
			vq.enable_notifs();
		}
	}

	pub fn disable_notifs(&self) {
		if let Some(ref vq) = self.vq {
			vq.disable_notifs();
		}
	}

	fn poll(&self) {
		if let Some(ref vq) = self.vq {
			vq.poll();
		}
	}

	/// Returns either a BufferToken and the corresponding index of the
	/// virtqueue it is coming from. (Index in the TxQueues.vqs vector)
	///
	/// OR returns None, if no BufferToken could be generated
	fn get_tkn(&mut self, len: usize) -> Option<(BufferToken, usize)> {
		// Check all ready token, for correct size.
		// Drop token if not so
		//
		// All Tokens inside the ready_queue are coming from the main queue with index 0.
		while let Some(mut tkn) = self.ready_queue.pop() {
			let (send_len, _) = tkn.len();

			match send_len.cmp(&len) {
				Ordering::Less => {}
				Ordering::Equal => return Some((tkn, 0)),
				Ordering::Greater => {
					tkn.restr_size(Some(len), None).unwrap();
					return Some((tkn, 0));
				}
			}
		}

		if self.poll_receiver.is_empty() {
			self.poll();
		}

		while let Ok(mut buffer_token) = self.poll_receiver.try_recv() {
			buffer_token.reset();
			let (send_len, _) = buffer_token.len();

			match send_len.cmp(&len) {
				Ordering::Less => {}
				Ordering::Equal => return Some((buffer_token, 0)),
				Ordering::Greater => {
					buffer_token.restr_size(Some(len), None).unwrap();
					return Some((buffer_token, 0));
				}
			}
		}

		// As usize is currently safe as the minimal usize is defined as 16bit in rust.
		let spec = BuffSpec::Single(Bytes::new(len).unwrap());

		match BufferToken::new(Some(spec), None) {
			Ok(tkn) => Some((tkn, 0)),
			Err(_) => {
				// Here it is possible if multiple queues are enabled to get another buffertoken from them!
				// Info the queues are disabled upon initialization and should be enabled somehow!
				None
			}
		}
	}

	/// Provides a slice to copy the packet and transfer the packet
	/// to the send queue. The caller has to creatde the header
	/// for the vsock interface.
	pub fn send_packet<R, F>(&mut self, len: usize, f: F) -> R
	where
		F: FnOnce(&mut [u8]) -> R,
	{
		if let Some((mut buff_tkn, _vq_index)) = self.get_tkn(len) {
			let (send_ptrs, _) = buff_tkn.raw_ptrs();
			let (buff_ptr, _) = send_ptrs.unwrap()[0];

			let buf_slice: &'static mut [u8] =
				unsafe { core::slice::from_raw_parts_mut(buff_ptr, len) };
			let result = f(buf_slice);

			if let Some(ref vq) = self.vq {
				vq.dispatch_await(
					buff_tkn,
					self.poll_sender.clone(),
					false,
					BufferType::Direct,
				)
				.unwrap();

				result
			} else {
				panic!("Unable to get token for send queue");
			}
		} else {
			panic!("Unable to get send queue");
		}
	}
}

pub(crate) struct EventQueue {
	vq: Option<Rc<dyn Virtq>>,
	poll_sender: async_channel::Sender<BufferToken>,
	poll_receiver: async_channel::Receiver<BufferToken>,
}

impl EventQueue {
	pub fn new() -> Self {
		let (poll_sender, poll_receiver) = async_channel::unbounded();

		Self {
			vq: None,
			poll_sender,
			poll_receiver,
		}
	}

	pub fn add(&mut self, vq: Rc<dyn Virtq>) {
		let num_buff: u16 = vq.size().into();
		let event_size = mem::size_of::<Event>()
			.align_up(core::mem::size_of::<crossbeam_utils::CachePadded<u8>>());
		let spec = BuffSpec::Single(Bytes::new(event_size).unwrap());

		for _ in 0..num_buff {
			let buff_tkn = match BufferToken::new(None, Some(spec.clone())) {
				Ok(tkn) => tkn,
				Err(_vq_err) => {
					error!("Setup of vsock queue failed, which should not happen!");
					panic!("setup of vsock queue failed!");
				}
			};

			// BufferTokens are directly provided to the queue
			// TransferTokens are directly dispatched
			// Transfers will be awaited at the queue
			match vq.dispatch_await(
				buff_tkn,
				self.poll_sender.clone(),
				false,
				BufferType::Direct,
			) {
				Ok(_) => (),
				Err(_) => {
					error!("Descriptor IDs were exhausted earlier than expected.");
					break;
				}
			}
		}

		self.vq = Some(vq);
	}

	pub fn enable_notifs(&self) {
		if let Some(ref vq) = self.vq {
			vq.enable_notifs();
		}
	}

	pub fn disable_notifs(&self) {
		if let Some(ref vq) = self.vq {
			vq.disable_notifs();
		}
	}
}

/// A wrapper struct for the raw configuration structure.
/// Handling the right access to fields, as some are read-only
/// for the driver.
pub(crate) struct VsockDevCfg {
	pub raw: &'static VsockDevCfgRaw,
	pub dev_id: u16,
	pub features: virtio::vsock::F,
}

pub(crate) struct VirtioVsockDriver {
	pub(super) dev_cfg: VsockDevCfg,
	pub(super) com_cfg: ComCfg,
	pub(super) isr_stat: IsrStatus,
	pub(super) notif_cfg: NotifCfg,
	pub(super) irq: InterruptLine,

	pub(super) event_vq: EventQueue,
	pub(super) recv_vq: RxQueue,
	pub(super) send_vq: TxQueue,
}

impl VirtioVsockDriver {
	#[cfg(feature = "pci")]
	pub fn get_dev_id(&self) -> u16 {
		self.dev_cfg.dev_id
	}

	#[inline]
	pub fn get_cid(&self) -> u64 {
		self.dev_cfg.raw.guest_cid
	}

	#[cfg(feature = "pci")]
	pub fn set_failed(&mut self) {
		self.com_cfg.set_failed();
	}

	pub fn disable_interrupts(&self) {
		// For send and receive queues?
		// Only for receive? Because send is off anyway?
		self.recv_vq.disable_notifs();
	}

	pub fn enable_interrupts(&self) {
		// For send and receive queues?
		// Only for receive? Because send is off anyway?
		self.recv_vq.enable_notifs();
	}

	pub fn handle_interrupt(&mut self) {
		let _ = self.isr_stat.is_interrupt();

		if self.isr_stat.is_cfg_change() {
			info!("Configuration changes are not possible! Aborting");
			todo!("Implement possibility to change config on the fly...")
		}

		self.isr_stat.acknowledge();
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
	pub fn init_dev(&mut self) -> Result<(), VirtioVsockError> {
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
		self.recv_vq.add(Rc::new(
			SplitVq::new(
				&mut self.com_cfg,
				&self.notif_cfg,
				VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
				VqIndex::from(0u16),
				self.dev_cfg.features.into(),
			)
			.unwrap(),
		));
		// Interrupt for receiving packets is wanted
		self.recv_vq.enable_notifs();

		self.send_vq.add(Rc::new(
			SplitVq::new(
				&mut self.com_cfg,
				&self.notif_cfg,
				VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
				VqIndex::from(1u16),
				self.dev_cfg.features.into(),
			)
			.unwrap(),
		));
		// Interrupt for communicating that a sended packet left, is not needed
		self.send_vq.disable_notifs();

		// create the queues and tell device about them
		self.event_vq.add(Rc::new(
			SplitVq::new(
				&mut self.com_cfg,
				&self.notif_cfg,
				VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
				VqIndex::from(2u16),
				self.dev_cfg.features.into(),
			)
			.unwrap(),
		));
		// Interrupt for event packets is wanted
		self.event_vq.enable_notifs();

		// At this point the device is "live"
		self.com_cfg.drv_ok();

		Ok(())
	}

	#[inline]
	pub fn process_packet<F>(&mut self, f: F)
	where
		F: FnMut(&Hdr, &[u8]),
	{
		self.recv_vq.process_packet(f)
	}

	/// Provides a slice to copy the packet and transfer the packet
	/// to the send queue. The caller has to creatde the header
	/// for the vsock interface.
	#[inline]
	pub fn send_packet<R, F>(&mut self, len: usize, f: F) -> R
	where
		F: FnOnce(&mut [u8]) -> R,
	{
		self.send_vq.send_packet(len, f)
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
