#![allow(dead_code)]

cfg_if::cfg_if! {
	if #[cfg(feature = "pci")] {
		mod pci;
	} else {
		mod mmio;
	}
}

use alloc::vec::Vec;

use hermit_sync::without_interrupts;
use smallvec::SmallVec;
use virtio::FeatureBits;
use virtio::console::Config;
use volatile::VolatileRef;
use volatile::access::ReadOnly;

use crate::VIRTIO_MAX_QUEUE_SIZE;
use crate::drivers::error::DriverError;
use crate::drivers::virtio::error::VirtioConsoleError;
#[cfg(not(feature = "pci"))]
use crate::drivers::virtio::transport::mmio::{ComCfg, IsrStatus, NotifCfg};
#[cfg(feature = "pci")]
use crate::drivers::virtio::transport::pci::{ComCfg, IsrStatus, NotifCfg};
use crate::drivers::virtio::virtqueue::split::SplitVq;
use crate::drivers::virtio::virtqueue::{
	AvailBufferToken, BufferElem, BufferType, UsedBufferToken, VirtQueue, Virtq, VqIndex, VqSize,
};
use crate::drivers::{Driver, InterruptLine};
use crate::mm::device_alloc::DeviceAlloc;

fn fill_queue(vq: &mut VirtQueue, num_packets: u16, packet_size: u32) {
	for _ in 0..num_packets {
		let buff_tkn = match AvailBufferToken::new(
			{
				let mut vec = SmallVec::new();
				vec.push(BufferElem::Vector(Vec::with_capacity_in(
					packet_size.try_into().unwrap(),
					DeviceAlloc,
				)));
				vec
			},
			SmallVec::new(),
		) {
			Ok(tkn) => tkn,
			Err(_vq_err) => {
				panic!("Setup of console queue failed, which should not happen!");
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

			packet_size: crate::CONSOLE_PACKET_SIZE,
		}
	}

	pub fn add(&mut self, mut vq: VirtQueue) {
		const BUFF_PER_PACKET: u16 = 1;
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
		F: FnMut(&[u8]),
	{
		while let Some(mut buffer_tkn) = self.get_next() {
			let packet = buffer_tkn.used_recv_buff.pop_front_vec().unwrap();

			if let Some(ref mut vq) = self.vq {
				f(&packet[..]);

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
			packet_length: crate::CONSOLE_PACKET_SIZE,
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
	pub fn send_packet(&mut self, buf: &[u8]) {
		// We need to poll to get the queue to remove elements from the table and make space for
		// what we are about to add
		self.poll();
		if let Some(ref mut vq) = self.vq {
			assert!(buf.len() < usize::try_from(self.packet_length).unwrap());
			let mut packet = Vec::with_capacity_in(buf.len(), DeviceAlloc);
			packet.extend_from_slice(buf);

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
		} else {
			panic!("Unable to get send queue");
		}
	}
}

/// A wrapper struct for the raw configuration structure.
/// Handling the right access to fields, as some are read-only
/// for the driver.
pub(crate) struct ConsoleDevCfg {
	pub raw: VolatileRef<'static, Config, ReadOnly>,
	pub dev_id: u16,
	pub features: virtio::console::F,
}

pub(crate) struct VirtioConsoleDriver {
	pub(super) dev_cfg: ConsoleDevCfg,
	pub(super) com_cfg: ComCfg,
	pub(super) isr_stat: IsrStatus,
	pub(super) notif_cfg: NotifCfg,
	pub(super) irq: InterruptLine,

	pub(super) recv_vq: RxQueue,
	pub(super) send_vq: TxQueue,
}

impl Driver for VirtioConsoleDriver {
	fn get_interrupt_number(&self) -> InterruptLine {
		self.irq
	}

	fn get_name(&self) -> &'static str {
		"virtio"
	}
}

impl VirtioConsoleDriver {
	pub fn write(&mut self, buf: &[u8]) -> Result<(), DriverError> {
		without_interrupts(|| {
			self.send_vq.send_packet(buf);
		});

		Ok(())
	}

	pub fn read(&mut self) -> Result<Option<u8>, DriverError> {
		// Logic to read data from the console
		Ok(None)
	}

	/// Handle interrupt and acknowledge interrupt
	pub fn handle_interrupt(&mut self) {
		let status = self.isr_stat.is_queue_interrupt();

		debug!("Virtion console receive interrupt!");

		#[cfg(not(feature = "pci"))]
		if status.contains(virtio::mmio::InterruptStatus::CONFIGURATION_CHANGE_NOTIFICATION) {
			info!("Configuration changes are not possible! Aborting");
			todo!("Implement possibility to change config on the fly...");
		}

		#[cfg(feature = "pci")]
		if status.contains(virtio::pci::IsrStatus::DEVICE_CONFIGURATION_INTERRUPT) {
			info!("Configuration changes are not possible! Aborting");
			todo!("Implement possibility to change config on the fly...");
		}

		self.isr_stat.acknowledge();
	}

	#[cfg(feature = "pci")]
	pub fn set_failed(&mut self) {
		self.com_cfg.set_failed();
	}

	/// Negotiates a subset of features, understood and wanted by both the OS
	/// and the device.
	fn negotiate_features(
		&mut self,
		driver_features: virtio::console::F,
	) -> Result<(), VirtioConsoleError> {
		let device_features = virtio::console::F::from(self.com_cfg.dev_features());

		if device_features.requirements_satisfied() {
			info!("Feature set wanted by console driver are in conformance with specification.");
		} else {
			return Err(VirtioConsoleError::FeatureRequirementsNotMet(
				device_features,
			));
		}

		if device_features.contains(driver_features) {
			// If device supports subset of features write feature set to common config
			self.com_cfg.set_drv_features(driver_features.into());
			Ok(())
		} else {
			Err(VirtioConsoleError::IncompatibleFeatureSets(
				driver_features,
				device_features,
			))
		}
	}

	pub fn init_dev(&mut self) -> Result<(), VirtioConsoleError> {
		// Reset
		self.com_cfg.reset_dev();

		// Indicate device, that OS noticed it
		self.com_cfg.ack_dev();

		// Indicate device, that driver is able to handle it
		self.com_cfg.set_drv();

		let features = virtio::console::F::VERSION_1;
		self.negotiate_features(features)?;

		// Indicates the device, that the current feature set is final for the driver
		// and will not be changed.
		self.com_cfg.features_ok();

		// Checks if the device has accepted final set. This finishes feature negotiation.
		if self.com_cfg.check_features() {
			info!(
				"Features have been negotiated between virtio console device {:x} and driver.",
				self.dev_cfg.dev_id
			);
			// Set feature set in device config fur future use.
			self.dev_cfg.features = features;
		} else {
			return Err(VirtioConsoleError::FailFeatureNeg(self.dev_cfg.dev_id));
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
		// Interrupt for communicating that a sent packet left, is not needed
		self.send_vq.disable_notifs();

		// At this point the device is "live"
		self.com_cfg.drv_ok();

		Ok(())
	}
}

/// Error module of virtio console device driver.
pub mod error {
	/// Virtio console device error enum.
	#[derive(Debug, Copy, Clone)]
	pub enum VirtioConsoleError {
		#[cfg(feature = "pci")]
		NoDevCfg(u16),
		/// The device did not acknowledge the negotiated feature set.
		FailFeatureNeg(u16),
		/// Set of features does not adhere to the requirements of features
		/// indicated by the specification
		FeatureRequirementsNotMet(virtio::console::F),
		/// The first u64 contains the feature bits wanted by the driver.
		/// but which are incompatible with the device feature set, second u64.
		IncompatibleFeatureSets(virtio::console::F, virtio::console::F),
	}
}
