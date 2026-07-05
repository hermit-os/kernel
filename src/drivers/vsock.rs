#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::vec::Vec;

use pci_types::InterruptLine;
use smallvec::SmallVec;
use virtio::vsock::{ConfigVolatileFieldAccess, Hdr};
use volatile::VolatileRef;
use volatile::access::ReadOnly;

use super::virtio::virtqueue::VirtQueue;
use crate::config::{VIRTIO_MAX_QUEUE_SIZE, VSOCK_PACKET_SIZE};
#[cfg(not(feature = "pci"))]
use crate::drivers::mmio::get_vsock_driver;
#[cfg(feature = "pci")]
use crate::drivers::pci::get_vsock_driver;
use crate::drivers::virtio::ControlRegisters;
use crate::drivers::virtio::error::VirtioVsockError;
use crate::drivers::virtio::transport::{InterruptCapability, UniCapsColl};
use crate::drivers::virtio::virtqueue::split::SplitVq;
use crate::drivers::virtio::virtqueue::{
	AvailBufferToken, BufferElem, BufferType, UsedBufferToken, Virtq,
};
use crate::drivers::{Driver, InterruptHandlerMap};
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

			packet_size: VSOCK_PACKET_SIZE,
		}
	}

	pub fn add(&mut self, mut vq: VirtQueue) {
		const BUFF_PER_PACKET: u16 = 2;
		let num_packets = vq.size() / BUFF_PER_PACKET;
		info!("num_packets {num_packets}");
		fill_queue(&mut vq, num_packets, self.packet_size);

		self.vq = Some(vq);
	}

	pub fn enable_notifs(&mut self) {
		let Some(vq) = &mut self.vq else {
			return;
		};

		vq.enable_notifs();
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

	pub fn process_packet<F>(&mut self, mut f: F)
	where
		F: FnMut(&Hdr, &[u8]),
	{
		while let Some(mut buffer_tkn) = self.get_next() {
			let header = unsafe {
				buffer_tkn
					.used_recv_buff
					.pop_front_downcast::<Hdr>()
					.unwrap()
			};
			let packet = buffer_tkn.used_recv_buff.pop_front_vec().unwrap();

			let vq = self.vq.as_mut().expect("Invalid length of receive queue");

			f(&header, &packet[..]);

			fill_queue(vq, 1, self.packet_size);
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
			packet_length: VSOCK_PACKET_SIZE + size_of::<Hdr>() as u32,
		}
	}

	pub fn add(&mut self, vq: VirtQueue) {
		self.vq = Some(vq);
	}

	pub fn enable_notifs(&mut self) {
		let Some(vq) = &mut self.vq else {
			return;
		};

		vq.enable_notifs();
	}

	pub fn disable_notifs(&mut self) {
		let Some(vq) = &mut self.vq else {
			return;
		};

		vq.disable_notifs();
	}

	fn poll(&mut self) {
		let Some(vq) = &mut self.vq else {
			return;
		};

		while vq.try_recv().is_ok() {}
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
		let vq = self.vq.as_mut().expect("Unable to get send queue");

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
		let num_packets = vq.size() / BUFF_PER_PACKET;
		fill_queue(&mut vq, num_packets, self.packet_size);
		self.vq = Some(vq);
	}

	pub fn enable_notifs(&mut self) {
		let Some(vq) = &mut self.vq else {
			return;
		};

		vq.enable_notifs();
	}

	pub fn disable_notifs(&mut self) {
		let Some(vq) = &mut self.vq else {
			return;
		};

		vq.disable_notifs();
	}
}

/// A wrapper struct for the raw configuration structure.
/// Handling the right access to fields, as some are read-only
/// for the driver.
pub(crate) struct VsockDevCfg {
	pub raw: VolatileRef<'static, virtio::vsock::Config, ReadOnly>,
	pub features: virtio::vsock::F,
}

pub(crate) struct VirtioVsockDriver {
	pub(super) dev_cfg: VsockDevCfg,
	pub(super) caps_coll: UniCapsColl,

	pub(super) event_vq: EventQueue,
	pub(super) recv_vq: RxQueue,
	pub(super) send_vq: TxQueue,
}

impl Driver for VirtioVsockDriver {
	fn get_name() -> &'static str {
		"virtio-vsock"
	}
}

impl VirtioVsockDriver {
	#[inline]
	pub fn get_cid(&self) -> u64 {
		self.dev_cfg.raw.as_ptr().guest_cid().read().to_ne()
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

impl super::virtio::VirtioDriver for VirtioVsockDriver {
	type Config = virtio::vsock::Config;
	type Error = VirtioVsockError;

	/// Initializes the device in adherence to specification. Returns Some(VirtioVsockError)
	/// upon failure and None in case everything worked as expected.
	///
	/// See Virtio specification v1.1. - 3.1.1.
	///                      and v1.1. - 5.10.6
	fn init_dev(
		(mut caps_coll, dev_cfg_raw): (
			UniCapsColl,
			VolatileRef<'static, virtio::vsock::Config, ReadOnly>,
		),
		handlers: &mut InterruptHandlerMap,
		irq: Option<InterruptLine>,
	) -> Result<Self, (VirtioVsockError, UniCapsColl)> {
		// Reset
		caps_coll.com_cfg.reset_dev();

		// Indicate device, that OS noticed it
		caps_coll.com_cfg.ack_dev();

		// Indicate device, that driver is able to handle it
		caps_coll.com_cfg.set_drv();

		let minimal_features = virtio::vsock::F::VERSION_1;
		let negotiated_features = caps_coll
			.com_cfg
			.control_registers()
			.negotiate_features(minimal_features);

		if !negotiated_features.contains(minimal_features) {
			error!("Device features set, does not satisfy minimal features needed. Aborting!");
			return Err((VirtioVsockError::FailFeatureNeg, caps_coll));
		}

		// Indicates the device, that the current feature set is final for the driver
		// and will not be changed.
		caps_coll.com_cfg.features_ok();

		// Checks if the device has accepted final set. This finishes feature negotiation.
		let dev_cfg = if caps_coll.com_cfg.check_features() {
			info!("Features have been negotiated between virtio socket device and driver.",);
			// Set feature set in device config fur future use.
			VsockDevCfg {
				raw: dev_cfg_raw,
				features: negotiated_features,
			}
		} else {
			error!("The device does not support our subset of features.");
			return Err((VirtioVsockError::FailFeatureNeg, caps_coll));
		};

		let mut recv_vq = RxQueue::new();
		// create the queues and tell device about them
		recv_vq.add(VirtQueue::Split(
			SplitVq::new(
				&mut caps_coll.com_cfg,
				&caps_coll.notif_cfg,
				VIRTIO_MAX_QUEUE_SIZE,
				0,
				dev_cfg.features.into(),
			)
			.unwrap(),
		));
		// Interrupt for receiving packets is wanted
		recv_vq.enable_notifs();

		let mut send_vq = TxQueue::new();
		send_vq.add(VirtQueue::Split(
			SplitVq::new(
				&mut caps_coll.com_cfg,
				&caps_coll.notif_cfg,
				VIRTIO_MAX_QUEUE_SIZE,
				1,
				dev_cfg.features.into(),
			)
			.unwrap(),
		));
		// Interrupt for communicating that a sent packet left, is not needed
		send_vq.disable_notifs();

		let mut event_vq = EventQueue::new();
		// create the queues and tell device about them
		event_vq.add(VirtQueue::Split(
			SplitVq::new(
				&mut caps_coll.com_cfg,
				&caps_coll.notif_cfg,
				VIRTIO_MAX_QUEUE_SIZE,
				2,
				dev_cfg.features.into(),
			)
			.unwrap(),
		));

		match &mut caps_coll.int_cap {
			InterruptCapability::IsrStatus(_) => {
				let irq = irq.unwrap();
				handlers.entry(irq).or_default().push_back(|| {
					if let Some(driver) = get_vsock_driver() {
						driver.lock().handle_interrupt();
					};
				});
				crate::arch::kernel::interrupts::add_irq_name(irq, "virtio");
				info!("Virtio interrupt handler at line {irq}");
			}
			#[cfg(all(feature = "pci", target_arch = "x86_64"))]
			InterruptCapability::Msix(msix_table) => {
				caps_coll.com_cfg.register_msix_vectors(
					msix_table,
					handlers,
					|| {
						if let Some(driver) = get_vsock_driver() {
							driver.lock().handle_device_configuration_interrupt();
						};
					},
					// The no-op handler allows the processor to receive an interrupt and reschedule.
					// FIXME: replace with a function to wake the vsock task waker once it is not woken unconditionally.
					[([0], (|| {}) as fn())].into_iter(),
					1..3,
				);
			}
		}

		// Interrupt for event packets is wanted
		event_vq.enable_notifs();

		// At this point the device is "live"
		caps_coll.com_cfg.drv_ok();

		Ok(Self {
			dev_cfg,
			caps_coll,
			event_vq,
			recv_vq,
			send_vq,
		})
	}

	#[cfg(feature = "pci")]
	fn no_dev_cfg_err(dev_id: u16) -> Self::Error {
		VirtioVsockError::NoDevCfg(dev_id)
	}
}

impl VirtioVsockDriver {
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
	use thiserror::Error;

	/// Virtio socket device error enum.
	#[derive(Error, Debug, Copy, Clone)]
	pub enum VirtioVsockError {
		#[error(
			"Virtio socket device driver failed, for device {0:x}, due to a missing or malformed device config!"
		)]
		NoDevCfg(u16),

		#[error(
			"Virtio socket device driver failed, for device {0:x}, due to a missing or malformed common config!"
		)]
		NoComCfg(u16),

		#[error(
			"Virtio socket device driver failed, for device {0:x}, due to a missing or malformed ISR status config!"
		)]
		NoIsrCfg(u16),

		#[error(
			"Virtio socket device driver failed, for device {0:x}, due to a missing or malformed notification config!"
		)]
		NoNotifCfg(u16),

		#[error(
			"Virtio socket device driver failed, device did not acknowledge negotiated feature set!"
		)]
		FailFeatureNeg,
	}
}
