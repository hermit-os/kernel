#![allow(dead_code)]

#[cfg(feature = "pci")]
pub mod pci;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::mem;

use pci_types::InterruptLine;
use smallvec::SmallVec;
use virtio::FeatureBits;
use virtio::vsock::Hdr;

use super::virtio::virtqueue::VirtQueue;
use crate::config::VIRTIO_MAX_QUEUE_SIZE;
use crate::drivers::Driver;
use crate::drivers::virtio::error::VirtioVsockError;
#[cfg(feature = "pci")]
use crate::drivers::virtio::transport::pci::{ComCfg, IsrStatus, NotifCfg};
use crate::drivers::virtio::virtqueue::split::SplitVq;
use crate::drivers::virtio::virtqueue::{
	AvailBufferToken, BufferElem, BufferType, UsedBufferToken, Virtq, VqIndex, VqSize,
};
#[cfg(feature = "pci")]
use crate::drivers::vsock::pci::VsockDevCfgRaw;
use crate::mm::device_alloc::DeviceAlloc;

fn fill_queue(vq: &mut VirtQueue, num_packets: u16, packet_size: u32) {
	for _ in 0..num_packets {
		let buff_tkn = match AvailBufferToken::new(
			SmallVec::new(),
			SmallVec::from_buf([
				BufferElem::Sized(Box::<Hdr, _>::new_uninit_in(DeviceAlloc)),
				BufferElem::Vector(Vec::with_capacity_in(
					packet_size.try_into().unwrap(),
					DeviceAlloc,
				)),
			]),
		) {
			Ok(tkn) => tkn,
			Err(_vq_err) => {
				error!("Setup of network queue failed, which should not happen!");
				panic!("setup of network queue failed!");
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

			packet_size: crate::VSOCK_PACKET_SIZE,
		}
	}

	pub fn add(&mut self, mut vq: VirtQueue) {
		const BUFF_PER_PACKET: u16 = 2;
		let num_packets: u16 = u16::from(vq.size()) / BUFF_PER_PACKET;
		info!("num_packets {num_packets}");
		fill_queue(&mut vq, num_packets, self.packet_size);

		self.vq = Some(vq);
	}

	pub fn enable_notifs(&mut self) {
		if let Some(ref mut vq) = self.vq {
			vq.enable_notifs();
		}
	}

	pub fn disable_notifs(&mut self) {
		if let Some(ref mut vq) = self.vq {
			vq.disable_notifs();
		}
	}

	fn get_next(&mut self) -> Option<UsedBufferToken> {
		self.vq.as_mut().unwrap().try_recv().ok()
	}

	pub fn process_packet<F>(&mut self, mut f: F)
	where
		F: FnMut(&Hdr, &[u8]),
	{
		while let Some(mut buffer_tkn) = self.get_next() {
			let header = buffer_tkn
				.used_recv_buff
				.pop_front_downcast::<Hdr>()
				.unwrap();
			let packet = buffer_tkn.used_recv_buff.pop_front_vec().unwrap();

			if let Some(ref mut vq) = self.vq {
				f(&header, &packet[..]);

				fill_queue(vq, 1, self.packet_size);
			} else {
				panic!("Invalid length of receive queue");
			}
		}
	}
}

pub(crate) struct TxQueue {
	vq: Option<VirtQueue>,
	/// Indicates, whether the Driver/Device are using multiple
	/// queues for communication.
	packet_length: u32,
}

impl TxQueue {
	pub fn new() -> Self {
		Self {
			vq: None,
			packet_length: crate::VSOCK_PACKET_SIZE + mem::size_of::<Hdr>() as u32,
		}
	}

	pub fn add(&mut self, vq: VirtQueue) {
		self.vq = Some(vq);
	}

	pub fn enable_notifs(&mut self) {
		if let Some(ref mut vq) = self.vq {
			vq.enable_notifs();
		}
	}

	pub fn disable_notifs(&mut self) {
		if let Some(ref mut vq) = self.vq {
			vq.disable_notifs();
		}
	}

	fn poll(&mut self) {
		if let Some(ref mut vq) = self.vq {
			while vq.try_recv().is_ok() {}
		}
	}

	/// Provides a slice to copy the packet and transfer the packet
	/// to the send queue. The caller has to create the header
	/// for the vsock interface.
	pub fn send_packet<R, F>(&mut self, len: usize, f: F) -> R
	where
		F: FnOnce(&mut [u8]) -> R,
	{
		// We need to poll to get the queue to remove elements from the table and make space for
		// what we are about to add
		self.poll();
		if let Some(ref mut vq) = self.vq {
			assert!(len < usize::try_from(self.packet_length).unwrap());
			let mut packet = Vec::with_capacity_in(len, DeviceAlloc);
			let result = unsafe {
				let result = f(packet.spare_capacity_mut().assume_init_mut());
				packet.set_len(len);
				result
			};

			let buff_tkn = AvailBufferToken::new(
				{
					let mut vec = SmallVec::new();
					vec.push(BufferElem::Vector(packet));
					vec
				},
				SmallVec::new(),
			)
			.unwrap();

			vq.dispatch(buff_tkn, false, BufferType::Direct).unwrap();

			result
		} else {
			panic!("Unable to get send queue");
		}
	}
}

pub(crate) struct EventQueue {
	vq: Option<VirtQueue>,
	packet_size: u32,
}

impl EventQueue {
	pub fn new() -> Self {
		Self {
			vq: None,
			packet_size: 128u32,
		}
	}

	/// Adds a given queue to the underlying vector and populates the queue with RecvBuffers.
	///
	/// Queues are all populated according to Virtio specification v1.1. - 5.1.6.3.1
	fn add(&mut self, mut vq: VirtQueue) {
		const BUFF_PER_PACKET: u16 = 2;
		let num_packets: u16 = u16::from(vq.size()) / BUFF_PER_PACKET;
		fill_queue(&mut vq, num_packets, self.packet_size);
		self.vq = Some(vq);
	}

	pub fn enable_notifs(&mut self) {
		if let Some(ref mut vq) = self.vq {
			vq.enable_notifs();
		}
	}

	pub fn disable_notifs(&mut self) {
		if let Some(ref mut vq) = self.vq {
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

impl Driver for VirtioVsockDriver {
	fn get_interrupt_number(&self) -> InterruptLine {
		self.irq
	}

	fn get_name(&self) -> &'static str {
		"virtio"
	}
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

	pub fn disable_interrupts(&mut self) {
		// For send and receive queues?
		// Only for receive? Because send is off anyway?
		self.recv_vq.disable_notifs();
	}

	pub fn enable_interrupts(&mut self) {
		// For send and receive queues?
		// Only for receive? Because send is off anyway?
		self.recv_vq.enable_notifs();
	}

	pub fn handle_interrupt(&mut self) {
		let status = self.isr_stat.is_queue_interrupt();

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

		// Indicate device, that OS noticed it
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
		self.recv_vq.add(VirtQueue::Split(
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

		self.send_vq.add(VirtQueue::Split(
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
		self.event_vq.add(VirtQueue::Split(
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
		self.recv_vq.process_packet(f);
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
		FeatureRequirementsNotMet(virtio::vsock::F),
		/// The first u64 contains the feature bits wanted by the driver.
		/// but which are incompatible with the device feature set, second u64.
		IncompatibleFeatureSets(virtio::vsock::F, virtio::vsock::F),
	}
}
