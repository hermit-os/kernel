//! A virtio-console driver.
//!
//! For details on the device, see [Console Device].
//! For details on the Rust definitions, see [`virtio::console`].
//!
//! [Console Device]: https://docs.oasis-open.org/virtio/virtio/v1.2/cs01/virtio-v1.2-cs01.html#x1-2900003

#![allow(dead_code)]

use alloc::vec::Vec;

use embedded_io::{ErrorType, Read, ReadReady, Write};
use smallvec::SmallVec;
use virtio::console::Config;
use volatile::VolatileRef;
use volatile::access::ReadOnly;

use crate::config::{CONSOLE_PACKET_SIZE, VIRTIO_MAX_QUEUE_SIZE};
use crate::drivers::error::DriverError;
#[cfg(not(feature = "pci"))]
use crate::drivers::mmio::get_console_driver;
#[cfg(feature = "pci")]
use crate::drivers::pci::get_console_driver;
use crate::drivers::virtio::error::{VirtioConsoleError, VirtioError};
use crate::drivers::virtio::transport::{InterruptCapability, UniCapsColl};
use crate::drivers::virtio::virtqueue::split::SplitVq;
use crate::drivers::virtio::virtqueue::{
	AvailBufferToken, BufferElem, BufferType, UsedBufferToken, VirtQueue, Virtq,
};
use crate::drivers::{Driver, InterruptHandlerMap, InterruptLine};
use crate::errno::Errno;
use crate::mm::device_alloc::DeviceAlloc;

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

pub(crate) struct VirtioUART;

impl VirtioUART {
	pub const fn new() -> Self {
		Self {}
	}
}

impl ErrorType for VirtioUART {
	type Error = Errno;
}

impl Read for VirtioUART {
	fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
		let drv = get_console_driver().ok_or(Errno::Io)?;

		drv.lock().read(buf)
	}
}

impl ReadReady for VirtioUART {
	fn read_ready(&mut self) -> Result<bool, Self::Error> {
		let Some(drv) = get_console_driver() else {
			return Ok(false);
		};

		Ok(drv.lock().has_packet())
	}
}

impl Write for VirtioUART {
	fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
		if let Some(drv) = get_console_driver() {
			drv.lock().write_all(buf)?;
		}

		Ok(buf.len())
	}

	fn flush(&mut self) -> Result<(), Self::Error> {
		Ok(())
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

			packet_size: CONSOLE_PACKET_SIZE,
		}
	}

	pub fn add(&mut self, mut vq: VirtQueue) {
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

	fn has_packet(&self) -> bool {
		self.vq.iter().any(|vq| vq.has_used_buffers())
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
			packet_length: CONSOLE_PACKET_SIZE,
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
	pub fn send_packet(&mut self, buf: &[u8]) {
		// We need to poll to get the queue to remove elements from the table and make space for
		// what we are about to add
		self.poll();
		let vq = self.vq.as_mut().unwrap();

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
	}
}

type ConsoleDevCfg = super::virtio::DevCfg<VirtioConsoleDriver>;

pub(crate) struct VirtioConsoleDriver {
	pub(super) dev_cfg: ConsoleDevCfg,
	pub(super) caps_coll: UniCapsColl,

	pub(super) recv_vq: RxQueue,
	pub(super) send_vq: TxQueue,
}

impl Driver for VirtioConsoleDriver {
	fn get_name() -> &'static str {
		"virtio-console"
	}
}

impl VirtioConsoleDriver {
	pub fn has_packet(&self) -> bool {
		self.recv_vq.has_packet()
	}

	/// Handle interrupt and acknowledge interrupt
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

		Self::handle_queue_interrupt();
	}

	fn handle_device_configuration_interrupt(&self) {
		if self.caps_coll.com_cfg.does_device_need_reset() {
			todo!("Device configuration change notification cannot be handled yet");
		}
	}

	fn handle_queue_interrupt() {
		crate::console::CONSOLE_WAKER.lock().wake();
	}
}

impl super::virtio::VirtioDriver for VirtioConsoleDriver {
	type Config = Config;
	type Error = VirtioConsoleError;
	type DeviceFeatures = virtio::console::F;

	const MINIMAL_FEATURES: Self::DeviceFeatures = virtio::console::F::VERSION_1;
	const OPTIONAL_FEATURES: Self::DeviceFeatures = virtio::console::F::empty();

	fn init_dev(
		(mut caps_coll, dev_cfg_raw): (UniCapsColl, VolatileRef<'static, Config, ReadOnly>),
		handlers: &mut InterruptHandlerMap,
		irq: Option<InterruptLine>,
	) -> Result<Self, (VirtioError, UniCapsColl)> {
		let mut recv_vq = RxQueue::new();
		let mut send_vq = TxQueue::new();

		let dev_cfg = match caps_coll.init_caps(dev_cfg_raw, |caps_coll, dev_cfg| {
			// create the queues and tell device about them
			recv_vq.add(VirtQueue::Split(
				SplitVq::new(
					&mut caps_coll.com_cfg,
					&caps_coll.notif_cfg,
					VIRTIO_MAX_QUEUE_SIZE,
					0,
					virtio::F::from(dev_cfg.features),
				)
				.unwrap(),
			));
			// Interrupt for receiving packets is wanted
			recv_vq.enable_notifs();

			send_vq.add(VirtQueue::Split(
				SplitVq::new(
					&mut caps_coll.com_cfg,
					&caps_coll.notif_cfg,
					VIRTIO_MAX_QUEUE_SIZE,
					1,
					virtio::F::from(dev_cfg.features),
				)
				.unwrap(),
			));
			// Interrupt for communicating that a sent packet left, is not needed
			send_vq.disable_notifs();

			match &mut caps_coll.int_cap {
				InterruptCapability::IsrStatus(_) => {
					let irq = irq.unwrap();
					handlers.entry(irq).or_default().push_back(|| {
						if let Some(driver) = get_console_driver() {
							driver.lock().handle_interrupt();
						};
					});
					crate::arch::kernel::interrupts::add_irq_name(irq, "virtio");
					info!("Virtio interrupt handler at line {irq}");
				}
				#[cfg(all(feature = "pci", target_arch = "x86_64"))]
				InterruptCapability::Msix(msix_table) => caps_coll.com_cfg.register_msix_vectors(
					msix_table,
					handlers,
					|| {
						if let Some(driver) = get_console_driver() {
							driver.lock().handle_device_configuration_interrupt();
						};
					},
					[(0..2u16, Self::handle_queue_interrupt as fn())].into_iter(),
					[],
				),
			}
			Ok(())
		}) {
			Ok(dev_cfg) => dev_cfg,
			Err(err) => return Err((err, caps_coll)),
		};

		Ok(Self {
			dev_cfg,
			caps_coll,
			recv_vq,
			send_vq,
		})
	}

	#[cfg(feature = "pci")]
	fn no_dev_cfg_err(dev_id: u16) -> Self::Error {
		VirtioConsoleError::NoDevCfg(dev_id)
	}
}

impl ErrorType for VirtioConsoleDriver {
	type Error = Errno;
}

impl Read for VirtioConsoleDriver {
	fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
		self.recv_vq
			.process_packet(|src| {
				buf[..src.len()].copy_from_slice(src);
				src.len()
			})
			.map_err(|_| Errno::Io)
	}
}

impl Write for VirtioConsoleDriver {
	fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
		self.send_vq.send_packet(buf);

		Ok(buf.len())
	}

	fn flush(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}
}

/// Error module of virtio console device driver.
pub mod error {
	use thiserror::Error;

	/// Virtio console device error enum.
	#[derive(Error, Debug, Copy, Clone)]
	pub enum VirtioConsoleError {
		#[cfg(feature = "pci")]
		#[error(
			"Virtio console device driver failed, for device {0:x}, due to a missing or malformed device config!"
		)]
		NoDevCfg(u16),
	}
}
