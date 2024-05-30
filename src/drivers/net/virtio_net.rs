//! A module containing a virtio network driver.
//!
//! The module contains ...

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cmp::Ordering;
use core::mem;

use align_address::Align;
use pci_types::InterruptLine;
use smoltcp::phy::{Checksum, ChecksumCapabilities};
use smoltcp::wire::{EthernetFrame, Ipv4Packet, Ipv6Packet, ETHERNET_HEADER_LEN};
use virtio_spec::net::{Hdr, HdrF};
use virtio_spec::FeatureBits;

use self::constants::{Status, MAX_NUM_VQ};
use self::error::VirtioNetError;
#[cfg(not(target_arch = "riscv64"))]
use crate::arch::kernel::core_local::increment_irq_counter;
use crate::config::VIRTIO_MAX_QUEUE_SIZE;
#[cfg(not(feature = "pci"))]
use crate::drivers::net::virtio_mmio::NetDevCfgRaw;
#[cfg(feature = "pci")]
use crate::drivers::net::virtio_pci::NetDevCfgRaw;
use crate::drivers::net::NetworkDriver;
#[cfg(not(feature = "pci"))]
use crate::drivers::virtio::transport::mmio::{ComCfg, IsrStatus, NotifCfg};
#[cfg(feature = "pci")]
use crate::drivers::virtio::transport::pci::{ComCfg, IsrStatus, NotifCfg};
use crate::drivers::virtio::virtqueue::packed::PackedVq;
use crate::drivers::virtio::virtqueue::split::SplitVq;
use crate::drivers::virtio::virtqueue::{BuffSpec, BufferToken, Bytes, Virtq, VqIndex, VqSize};
use crate::executor::device::{RxToken, TxToken};

/// A wrapper struct for the raw configuration structure.
/// Handling the right access to fields, as some are read-only
/// for the driver.
pub(crate) struct NetDevCfg {
	pub raw: &'static NetDevCfgRaw,
	pub dev_id: u16,
	pub features: virtio_spec::net::F,
}

pub struct CtrlQueue(Option<Rc<dyn Virtq>>);

impl CtrlQueue {
	pub fn new(vq: Option<Rc<dyn Virtq>>) -> Self {
		CtrlQueue(vq)
	}
}

#[allow(dead_code, non_camel_case_types)]
#[derive(Copy, Clone, Debug)]
#[repr(u8)]
enum CtrlClass {
	VIRTIO_NET_CTRL_RX = 1 << 0,
	VIRTIO_NET_CTRL_MAC = 1 << 1,
	VIRTIO_NET_CTRL_VLAN = 1 << 2,
	VIRTIO_NET_CTRL_ANNOUNCE = 1 << 3,
	VIRTIO_NET_CTRL_MQ = 1 << 4,
}

impl From<CtrlClass> for u8 {
	fn from(val: CtrlClass) -> Self {
		match val {
			CtrlClass::VIRTIO_NET_CTRL_RX => 1 << 0,
			CtrlClass::VIRTIO_NET_CTRL_MAC => 1 << 1,
			CtrlClass::VIRTIO_NET_CTRL_VLAN => 1 << 2,
			CtrlClass::VIRTIO_NET_CTRL_ANNOUNCE => 1 << 3,
			CtrlClass::VIRTIO_NET_CTRL_MQ => 1 << 4,
		}
	}
}

#[allow(dead_code, non_camel_case_types)]
#[derive(Copy, Clone, Debug)]
#[repr(u8)]
enum RxCmd {
	VIRTIO_NET_CTRL_RX_PROMISC = 1 << 0,
	VIRTIO_NET_CTRL_RX_ALLMULTI = 1 << 1,
	VIRTIO_NET_CTRL_RX_ALLUNI = 1 << 2,
	VIRTIO_NET_CTRL_RX_NOMULTI = 1 << 3,
	VIRTIO_NET_CTRL_RX_NOUNI = 1 << 4,
	VIRTIO_NET_CTRL_RX_NOBCAST = 1 << 5,
}

#[allow(dead_code, non_camel_case_types)]
#[derive(Copy, Clone, Debug)]
#[repr(u8)]
enum MacCmd {
	VIRTIO_NET_CTRL_MAC_TABLE_SET = 1 << 0,
	VIRTIO_NET_CTRL_MAC_ADDR_SET = 1 << 1,
}

#[allow(dead_code, non_camel_case_types)]
#[derive(Copy, Clone, Debug)]
#[repr(u8)]
enum VlanCmd {
	VIRTIO_NET_CTRL_VLAN_ADD = 1 << 0,
	VIRTIO_NET_CTRL_VLAN_DEL = 1 << 1,
}

#[allow(dead_code, non_camel_case_types)]
#[derive(Copy, Clone, Debug)]
#[repr(u8)]
enum AnceCmd {
	VIRTIO_NET_CTRL_ANNOUNCE_ACK = 1 << 0,
}

#[allow(dead_code, non_camel_case_types)]
#[derive(Copy, Clone, Debug)]
#[repr(u8)]
enum MqCmd {
	VIRTIO_NET_CTRL_MQ_VQ_PAIRS_SET = 1 << 0,
	VIRTIO_NET_CTRL_MQ_VQ_PAIRS_MIN = 1 << 1,
	VIRTIO_NET_CTRL_MQ_VQ_PAIRS_MAX = 0x80,
}

pub struct RxQueues {
	vqs: Vec<Rc<dyn Virtq>>,
	poll_sender: async_channel::Sender<Box<BufferToken>>,
	poll_receiver: async_channel::Receiver<Box<BufferToken>>,
	is_multi: bool,
}

impl RxQueues {
	pub fn new(vqs: Vec<Rc<dyn Virtq>>, is_multi: bool) -> Self {
		let (poll_sender, poll_receiver) = async_channel::unbounded();
		Self {
			vqs,
			poll_sender,
			poll_receiver,
			is_multi,
		}
	}

	/// Takes care if handling packets correctly which need some processing after being received.
	/// This currently include nothing. But in the future it might include among others::
	/// * Calculating missing checksums
	/// * Merging receive buffers, by simply checking the poll_queue (if VIRTIO_NET_F_MRG_BUF)
	fn post_processing(buffer_tkn: Box<BufferToken>) -> Result<Box<BufferToken>, VirtioNetError> {
		Ok(buffer_tkn)
	}

	/// Adds a given queue to the underlying vector and populates the queue with RecvBuffers.
	///
	/// Queues are all populated according to Virtio specification v1.1. - 5.1.6.3.1
	fn add(&mut self, vq: Rc<dyn Virtq>, dev_cfg: &NetDevCfg) {
		let num_buff: u16 = vq.size().into();

		let rx_size = if dev_cfg.features.contains(virtio_spec::net::F::MRG_RXBUF) {
			(1514 + mem::size_of::<Hdr>())
				.align_up(core::mem::size_of::<crossbeam_utils::CachePadded<u8>>())
		} else {
			dev_cfg.raw.get_mtu() as usize + mem::size_of::<Hdr>()
		};

		// See Virtio specification v1.1 - 5.1.6.3.1
		//
		let spec = BuffSpec::Single(Bytes::new(rx_size).unwrap());
		for _ in 0..num_buff {
			let buff_tkn = match vq.clone().prep_buffer(None, Some(spec.clone())) {
				Ok(tkn) => tkn,
				Err(_vq_err) => {
					error!("Setup of network queue failed, which should not happen!");
					panic!("setup of network queue failed!");
				}
			};

			// BufferTokens are directly provided to the queue
			// TransferTokens are directly dispatched
			// Transfers will be awaited at the queue
			buff_tkn
				.provide()
				.dispatch_await(self.poll_sender.clone(), false);
		}

		// Safe virtqueue
		self.vqs.push(vq);

		if self.vqs.len() > 1 {
			self.is_multi = true;
		}
	}

	fn get_next(&mut self) -> Option<Box<BufferToken>> {
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
		if self.is_multi {
			for vq in &self.vqs {
				vq.poll();
			}
		} else {
			self.vqs[0].poll();
		}
	}

	fn enable_notifs(&self) {
		if self.is_multi {
			for vq in &self.vqs {
				vq.enable_notifs();
			}
		} else {
			self.vqs[0].enable_notifs();
		}
	}

	fn disable_notifs(&self) {
		if self.is_multi {
			for vq in &self.vqs {
				vq.disable_notifs();
			}
		} else {
			self.vqs[0].disable_notifs();
		}
	}
}

/// Structure which handles transmission of packets and delegation
/// to the respective queue structures.
pub struct TxQueues {
	vqs: Vec<Rc<dyn Virtq>>,
	poll_sender: async_channel::Sender<Box<BufferToken>>,
	poll_receiver: async_channel::Receiver<Box<BufferToken>>,
	ready_queue: Vec<BufferToken>,
	/// Indicates, whether the Driver/Device are using multiple
	/// queues for communication.
	is_multi: bool,
}

impl TxQueues {
	pub fn new(vqs: Vec<Rc<dyn Virtq>>, ready_queue: Vec<BufferToken>, is_multi: bool) -> Self {
		let (poll_sender, poll_receiver) = async_channel::unbounded();
		Self {
			vqs,
			poll_sender,
			poll_receiver,
			ready_queue,
			is_multi,
		}
	}
	#[allow(dead_code)]
	fn enable_notifs(&self) {
		if self.is_multi {
			for vq in &self.vqs {
				vq.enable_notifs();
			}
		} else {
			self.vqs[0].enable_notifs();
		}
	}

	#[allow(dead_code)]
	fn disable_notifs(&self) {
		if self.is_multi {
			for vq in &self.vqs {
				vq.disable_notifs();
			}
		} else {
			self.vqs[0].disable_notifs();
		}
	}

	fn poll(&self) {
		if self.is_multi {
			for vq in &self.vqs {
				vq.poll();
			}
		} else {
			self.vqs[0].poll();
		}
	}

	fn add(&mut self, vq: Rc<dyn Virtq>, dev_cfg: &NetDevCfg) {
		// Safe virtqueue
		self.vqs.push(vq.clone());
		if self.vqs.len() == 1 {
			// Unwrapping is safe, as one virtq will be definitely in the vector.
			let vq = self.vqs.first().unwrap();

			if dev_cfg.features.contains(virtio_spec::net::F::GUEST_TSO4)
				| dev_cfg.features.contains(virtio_spec::net::F::GUEST_TSO6)
				| dev_cfg.features.contains(virtio_spec::net::F::GUEST_UFO)
			{
				// Virtio specification v1.1. - 5.1.6.2 point 5.
				//      Header and data are added as ONE output descriptor to the transmitvq.
				//      Hence we are interpreting this, as the fact, that send packets must be inside a single descriptor.
				// As usize is currently safe as the minimal usize is defined as 16bit in rust.
				let buff_def = Bytes::new(mem::size_of::<Hdr>() + 65550).unwrap();
				let spec = BuffSpec::Single(buff_def);

				let num_buff: u16 = vq.size().into();

				for _ in 0..num_buff {
					self.ready_queue.push(
						vq.clone()
							.prep_buffer(Some(spec.clone()), None)
							.unwrap()
							.write_seq(Some(&Hdr::default()), None::<&Hdr>)
							.unwrap(),
					)
				}
			} else {
				// Virtio specification v1.1. - 5.1.6.2 point 5.
				//      Header and data are added as ONE output descriptor to the transmitvq.
				//      Hence we are interpreting this, as the fact, that send packets must be inside a single descriptor.
				// As usize is currently safe as the minimal usize is defined as 16bit in rust.
				let buff_def =
					Bytes::new(mem::size_of::<Hdr>() + dev_cfg.raw.get_mtu() as usize).unwrap();
				let spec = BuffSpec::Single(buff_def);

				let num_buff: u16 = vq.size().into();

				for _ in 0..num_buff {
					self.ready_queue.push(
						vq.clone()
							.prep_buffer(Some(spec.clone()), None)
							.unwrap()
							.write_seq(Some(&Hdr::default()), None::<&Hdr>)
							.unwrap(),
					)
				}
			}
		} else {
			self.is_multi = true;
			// Currently we are doing nothing with the additional queues. They are inactive and might be used in the
			// future
		}
	}

	/// Returns either a buffertoken and the corresponding index of the
	/// virtqueue it is coming from. (Index in the TxQueues.vqs vector)
	///
	/// OR returns None, if no Buffertoken could be generated
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

		while let Ok(buffer_token) = self.poll_receiver.try_recv() {
			let mut tkn = buffer_token.reset();
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

		// As usize is currently safe as the minimal usize is defined as 16bit in rust.
		let spec = BuffSpec::Single(Bytes::new(len).unwrap());

		match self.vqs[0].clone().prep_buffer(Some(spec), None) {
			Ok(tkn) => Some((tkn, 0)),
			Err(_) => {
				// Here it is possible if multiple queues are enabled to get another buffertoken from them!
				// Info the queues are disabled upon initialization and should be enabled somehow!
				None
			}
		}
	}
}

/// Virtio network driver struct.
///
/// Struct allows to control devices virtqueues as also
/// the device itself.
pub(crate) struct VirtioNetDriver {
	pub(super) dev_cfg: NetDevCfg,
	pub(super) com_cfg: ComCfg,
	pub(super) isr_stat: IsrStatus,
	pub(super) notif_cfg: NotifCfg,

	pub(super) ctrl_vq: CtrlQueue,
	pub(super) recv_vqs: RxQueues,
	pub(super) send_vqs: TxQueues,

	pub(super) num_vqs: u16,
	#[cfg_attr(target_arch = "riscv64", allow(dead_code))]
	pub(super) irq: InterruptLine,
	pub(super) mtu: u16,
	pub(super) checksums: ChecksumCapabilities,
}

impl NetworkDriver for VirtioNetDriver {
	/// Returns the mac address of the device.
	/// If VIRTIO_NET_F_MAC is not set, the function panics currently!
	fn get_mac_address(&self) -> [u8; 6] {
		if self.dev_cfg.features.contains(virtio_spec::net::F::MAC) {
			self.dev_cfg.raw.get_mac()
		} else {
			unreachable!("Currently VIRTIO_NET_F_MAC must be negotiated!")
		}
	}

	/// Returns the current MTU of the device.
	fn get_mtu(&self) -> u16 {
		self.mtu
	}

	fn get_checksums(&self) -> ChecksumCapabilities {
		self.checksums.clone()
	}

	#[allow(dead_code)]
	fn has_packet(&self) -> bool {
		self.recv_vqs.poll();
		!self.recv_vqs.poll_receiver.is_empty()
	}

	/// Provides smoltcp a slice to copy the IP packet and transfer the packet
	/// to the send queue.
	fn send_packet<R, F>(&mut self, len: usize, f: F) -> R
	where
		F: FnOnce(&mut [u8]) -> R,
	{
		if let Some((mut buff_tkn, _vq_index)) =
			self.send_vqs.get_tkn(len + core::mem::size_of::<Hdr>())
		{
			let (send_ptrs, _) = buff_tkn.raw_ptrs();
			// Currently we have single Buffers in the TxQueue of size: MTU + ETHERNET_HEADER_LEN + VIRTIO_NET_HDR
			// see TxQueue.add()
			let (buff_ptr, _) = send_ptrs.unwrap()[0];

			// Do not show smoltcp the memory region for Hdr.
			let header = unsafe { &mut *(buff_ptr as *mut Hdr) };
			*header = Default::default();
			let buff_ptr =
				unsafe { buff_ptr.offset(isize::try_from(core::mem::size_of::<Hdr>()).unwrap()) };

			let buf_slice: &'static mut [u8] =
				unsafe { core::slice::from_raw_parts_mut(buff_ptr, len) };
			let result = f(buf_slice);

			// If a checksum isn't necessary, we have inform the host within the header
			// see Virtio specification 5.1.6.2
			if !self.checksums.tcp.tx() || !self.checksums.udp.tx() {
				header.flags = HdrF::NEEDS_CSUM;
				let ethernet_frame: smoltcp::wire::EthernetFrame<&[u8]> =
					EthernetFrame::new_unchecked(buf_slice);
				let packet_header_len: u16;
				let protocol;
				match ethernet_frame.ethertype() {
					smoltcp::wire::EthernetProtocol::Ipv4 => {
						let packet = Ipv4Packet::new_unchecked(ethernet_frame.payload());
						packet_header_len = packet.header_len().into();
						protocol = Some(packet.next_header());
					}
					smoltcp::wire::EthernetProtocol::Ipv6 => {
						let packet = Ipv6Packet::new_unchecked(ethernet_frame.payload());
						packet_header_len = packet.header_len().try_into().unwrap();
						protocol = Some(packet.next_header());
					}
					_ => {
						packet_header_len = 0;
						protocol = None;
					}
				}
				header.csum_start =
					(u16::try_from(ETHERNET_HEADER_LEN).unwrap() + packet_header_len).into();
				header.csum_offset = match protocol {
					Some(smoltcp::wire::IpProtocol::Tcp) => 16,
					Some(smoltcp::wire::IpProtocol::Udp) => 6,
					_ => 0,
				}
				.into();
			}

			buff_tkn
				.provide()
				.dispatch_await(self.send_vqs.poll_sender.clone(), false);

			result
		} else {
			panic!("Unable to get token for send queue");
		}
	}

	fn receive_packet(&mut self) -> Option<(RxToken, TxToken)> {
		match self.recv_vqs.get_next() {
			Some(transfer) => {
				let transfer = match RxQueues::post_processing(transfer) {
					Ok(trf) => trf,
					Err(vnet_err) => {
						warn!("Post processing failed. Err: {:?}", vnet_err);
						return None;
					}
				};

				let (_, recv_data_opt) = transfer.as_slices().unwrap();
				let mut recv_data = recv_data_opt.unwrap();

				// If the given length isn't 1, we currently fail.
				if recv_data.len() == 1 {
					let mut vec_data: Vec<u8> = Vec::with_capacity(self.mtu.into());
					let num_buffers = {
						const HEADER_SIZE: usize = mem::size_of::<Hdr>();
						let packet = recv_data.pop().unwrap();

						// drop packets with invalid packet size
						if packet.len() < HEADER_SIZE {
							transfer
								.reset()
								.provide()
								.dispatch_await(self.recv_vqs.poll_sender.clone(), false);

							return None;
						}

						let header = unsafe {
							core::mem::transmute::<[u8; HEADER_SIZE], Hdr>(
								packet[..HEADER_SIZE].try_into().unwrap(),
							)
						};
						trace!("Header: {:?}", header);
						let num_buffers = header.num_buffers;

						vec_data.extend_from_slice(&packet[mem::size_of::<Hdr>()..]);
						transfer
							.reset()
							.provide()
							.dispatch_await(self.recv_vqs.poll_sender.clone(), false);

						num_buffers
					};

					for _ in 1..num_buffers.to_ne() {
						let transfer =
							match RxQueues::post_processing(self.recv_vqs.get_next().unwrap()) {
								Ok(trf) => trf,
								Err(vnet_err) => {
									warn!("Post processing failed. Err: {:?}", vnet_err);
									return None;
								}
							};

						let (_, recv_data_opt) = transfer.as_slices().unwrap();
						let mut recv_data = recv_data_opt.unwrap();
						let packet = recv_data.pop().unwrap();
						vec_data.extend_from_slice(packet);
						transfer
							.reset()
							.provide()
							.dispatch_await(self.recv_vqs.poll_sender.clone(), false);
					}

					Some((RxToken::new(vec_data), TxToken::new()))
				} else {
					error!("Empty transfer, or with wrong buffer layout. Reusing and returning error to user-space network driver...");
					transfer
						.reset()
						.write_seq(None::<&Hdr>, Some(&Hdr::default()))
						.unwrap()
						.provide()
						.dispatch_await(self.recv_vqs.poll_sender.clone(), false);

					None
				}
			}
			None => None,
		}
	}

	fn set_polling_mode(&mut self, value: bool) {
		if value {
			self.disable_interrupts();
		} else {
			self.enable_interrupts();
		}
	}

	fn handle_interrupt(&mut self) -> bool {
		#[cfg(not(target_arch = "riscv64"))]
		increment_irq_counter(32 + self.irq);

		let result = if self.isr_stat.is_interrupt() {
			true
		} else if self.isr_stat.is_cfg_change() {
			info!("Configuration changes are not possible! Aborting");
			todo!("Implement possibility to change config on the fly...")
		} else {
			false
		};

		self.isr_stat.acknowledge();

		result
	}
}

// Backend-independent interface for Virtio network driver
impl VirtioNetDriver {
	#[cfg(feature = "pci")]
	pub fn get_dev_id(&self) -> u16 {
		self.dev_cfg.dev_id
	}

	#[cfg(feature = "pci")]
	pub fn set_failed(&mut self) {
		self.com_cfg.set_failed();
	}

	/// Returns the current status of the device, if VIRTIO_NET_F_STATUS
	/// has been negotiated. Otherwise assumes an active device.
	#[cfg(not(feature = "pci"))]
	pub fn dev_status(&self) -> u16 {
		if self.dev_cfg.features.contains(virtio_spec::net::F::STATUS) {
			self.dev_cfg.raw.get_status()
		} else {
			u16::from(Status::VIRTIO_NET_S_LINK_UP)
		}
	}

	/// Returns the links status.
	/// If feature VIRTIO_NET_F_STATUS has not been negotiated, then we assume the link is up!
	#[cfg(feature = "pci")]
	pub fn is_link_up(&self) -> bool {
		if self.dev_cfg.features.contains(virtio_spec::net::F::STATUS) {
			self.dev_cfg.raw.get_status() & u16::from(Status::VIRTIO_NET_S_LINK_UP)
				== u16::from(Status::VIRTIO_NET_S_LINK_UP)
		} else {
			true
		}
	}

	#[allow(dead_code)]
	pub fn is_announce(&self) -> bool {
		if self.dev_cfg.features.contains(virtio_spec::net::F::STATUS) {
			self.dev_cfg.raw.get_status() & u16::from(Status::VIRTIO_NET_S_ANNOUNCE)
				== u16::from(Status::VIRTIO_NET_S_ANNOUNCE)
		} else {
			false
		}
	}

	/// Returns the maximal number of virtqueue pairs allowed. This is the
	/// dominant setting to define the number of virtqueues for the network
	/// device and overrides the num_vq field in the common config.
	///
	/// Returns 1 (i.e. minimum number of pairs) if VIRTIO_NET_F_MQ is not set.
	#[allow(dead_code)]
	pub fn get_max_vq_pairs(&self) -> u16 {
		if self.dev_cfg.features.contains(virtio_spec::net::F::MQ) {
			self.dev_cfg.raw.get_max_virtqueue_pairs()
		} else {
			1
		}
	}

	pub fn disable_interrupts(&self) {
		// For send and receive queues?
		// Only for receive? Because send is off anyway?
		self.recv_vqs.disable_notifs();
	}

	pub fn enable_interrupts(&self) {
		// For send and receive queues?
		// Only for receive? Because send is off anyway?
		self.recv_vqs.enable_notifs();
	}

	/// Initializes the device in adherence to specification. Returns Some(VirtioNetError)
	/// upon failure and None in case everything worked as expected.
	///
	/// See Virtio specification v1.1. - 3.1.1.
	///                      and v1.1. - 5.1.5
	pub fn init_dev(&mut self) -> Result<(), VirtioNetError> {
		// Reset
		self.com_cfg.reset_dev();

		// Indicate device, that OS noticed it
		self.com_cfg.ack_dev();

		// Indicate device, that driver is able to handle it
		self.com_cfg.set_drv();

		let minimal_features = virtio_spec::net::F::VERSION_1 | virtio_spec::net::F::MAC;

		// If wanted, push new features into feats here:
		let mut features = minimal_features
			// Indirect descriptors can be used
			| virtio_spec::net::F::INDIRECT_DESC
			// Packed Vq can be used
			| virtio_spec::net::F::RING_PACKED
			| virtio_spec::net::F::NOTIFICATION_DATA
			// Host should avoid the creation of checksums
			| virtio_spec::net::F::CSUM
			// Guest avoids the creation of checksums
			| virtio_spec::net::F::GUEST_CSUM
			// MTU setting can be used
			| virtio_spec::net::F::MTU
			// Driver can merge receive buffers
			| virtio_spec::net::F::MRG_RXBUF
			// the link status can be announced
			| virtio_spec::net::F::STATUS
			// Multiqueue support
			| virtio_spec::net::F::MQ;

		// Currently the driver does NOT support the features below.
		// In order to provide functionality for these, the driver
		// needs to take care of calculating checksum in
		// RxQueues.post_processing()
		// | virtio_spec::net::F::GUEST_TSO4
		// | virtio_spec::net::F::GUEST_TSO6

		// Negotiate features with device. Automatically reduces selected feats in order to meet device capabilities.
		// Aborts in case incompatible features are selected by the driver or the device does not support min_feat_set.
		match self.negotiate_features(features) {
			Ok(_) => info!(
				"Driver found a subset of features for virtio device {:x}. Features are: {features:?}",
				self.dev_cfg.dev_id
			),
			Err(vnet_err) => {
				match vnet_err {
					VirtioNetError::FeatureRequirementsNotMet(features) => {
						error!("Network drivers feature set {features:?} does not satisfy rules in section 5.1.3.1 of specification v1.1. Aborting!");
						return Err(vnet_err);
					}
					VirtioNetError::IncompatibleFeatureSets(drv_feats, dev_feats) => {
						// Create a new matching feature set for device and driver if the minimal set is met!
						if !dev_feats.contains(minimal_features) {
							error!("Device features set, does not satisfy minimal features needed. Aborting!");
							return Err(VirtioNetError::FailFeatureNeg(self.dev_cfg.dev_id));
						} else {
							let common_features = drv_feats & dev_feats;
							if common_features.is_empty() {
								error!("Feature negotiation failed with minimal feature set. Aborting!");
								return Err(VirtioNetError::FailFeatureNeg(self.dev_cfg.dev_id));
							}
							features = common_features;

							match self.negotiate_features(features) {
                                Ok(_) => info!("Driver found a subset of features for virtio device {:x}. Features are: {features:?}", self.dev_cfg.dev_id),
                                Err(vnet_err) => {
                                    match vnet_err {
                                        VirtioNetError::FeatureRequirementsNotMet(features) => {
                                            error!("Network device offers a feature set {features:?} when used completely does not satisfy rules in section 5.1.3.1 of specification v1.1. Aborting!");
                                            return Err(vnet_err);
                                        },
                                        _ => {
                                            error!("Feature Set after reduction still not usable. Set: {features:?}. Aborting!");
                                            return Err(vnet_err);
                                        }
                                    }
                                }
                            }
						}
					}
					_ => {
						error!(
							"Wanted set of features is NOT supported by device. Set: {features:?}"
						);
						return Err(vnet_err);
					}
				}
			}
		}

		// Indicates the device, that the current feature set is final for the driver
		// and will not be changed.
		self.com_cfg.features_ok();

		// Checks if the device has accepted final set. This finishes feature negotiation.
		if self.com_cfg.check_features() {
			info!(
				"Features have been negotiated between virtio network device {:x} and driver.",
				self.dev_cfg.dev_id
			);
			// Set feature set in device config fur future use.
			self.dev_cfg.features = features;
		} else {
			return Err(VirtioNetError::FailFeatureNeg(self.dev_cfg.dev_id));
		}

		match self.dev_spec_init() {
			Ok(_) => info!(
				"Device specific initialization for Virtio network device {:x} finished",
				self.dev_cfg.dev_id
			),
			Err(vnet_err) => return Err(vnet_err),
		}
		// At this point the device is "live"
		self.com_cfg.drv_ok();

		if self.dev_cfg.features.contains(virtio_spec::net::F::CSUM)
			&& self
				.dev_cfg
				.features
				.contains(virtio_spec::net::F::GUEST_CSUM)
		{
			self.checksums.udp = Checksum::None;
			self.checksums.tcp = Checksum::None;
		} else if self.dev_cfg.features.contains(virtio_spec::net::F::CSUM) {
			self.checksums.udp = Checksum::Rx;
			self.checksums.tcp = Checksum::Rx;
		} else if self
			.dev_cfg
			.features
			.contains(virtio_spec::net::F::GUEST_CSUM)
		{
			self.checksums.udp = Checksum::Tx;
			self.checksums.tcp = Checksum::Tx;
		}
		debug!("{:?}", self.checksums);

		if self.dev_cfg.features.contains(virtio_spec::net::F::MTU) {
			self.mtu = self.dev_cfg.raw.get_mtu();
		}

		Ok(())
	}

	/// Negotiates a subset of features, understood and wanted by both the OS
	/// and the device.
	fn negotiate_features(
		&mut self,
		driver_features: virtio_spec::net::F,
	) -> Result<(), VirtioNetError> {
		let device_features = virtio_spec::net::F::from(self.com_cfg.dev_features());

		if device_features.requirements_satisfied() {
			info!("Feature set wanted by network driver are in conformance with specification.");
		} else {
			return Err(VirtioNetError::FeatureRequirementsNotMet(device_features));
		}

		if device_features.contains(driver_features) {
			// If device supports subset of features write feature set to common config
			self.com_cfg.set_drv_features(driver_features.into());
			Ok(())
		} else {
			Err(VirtioNetError::IncompatibleFeatureSets(
				driver_features,
				device_features,
			))
		}
	}

	/// Device Specific initialization according to Virtio specifictation v1.1. - 5.1.5
	fn dev_spec_init(&mut self) -> Result<(), VirtioNetError> {
		match self.virtqueue_init() {
			Ok(_) => info!("Network driver successfully initialized virtqueues."),
			Err(vnet_err) => return Err(vnet_err),
		}

		// Add a control if feature is negotiated
		if self.dev_cfg.features.contains(virtio_spec::net::F::CTRL_VQ) {
			if self
				.dev_cfg
				.features
				.contains(virtio_spec::net::F::RING_PACKED)
			{
				self.ctrl_vq = CtrlQueue(Some(Rc::new(
					PackedVq::new(
						&mut self.com_cfg,
						&self.notif_cfg,
						VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
						VqIndex::from(self.num_vqs),
						self.dev_cfg.features.into(),
					)
					.unwrap(),
				)));
			} else {
				self.ctrl_vq = CtrlQueue(Some(Rc::new(
					SplitVq::new(
						&mut self.com_cfg,
						&self.notif_cfg,
						VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
						VqIndex::from(self.num_vqs),
						self.dev_cfg.features.into(),
					)
					.unwrap(),
				)));
			}

			self.ctrl_vq.0.as_ref().unwrap().enable_notifs();
		}

		Ok(())
	}

	/// Initialize virtqueues via the queue interface and populates receiving queues
	fn virtqueue_init(&mut self) -> Result<(), VirtioNetError> {
		// We are assuming here, that the device single source of truth is the
		// device specific configuration. Hence we do NOT check if
		//
		// max_virtqueue_pairs + 1 < num_queues
		//
		// - the plus 1 is due to the possibility of an existing control queue
		// - the num_queues is found in the ComCfg struct of the device and defines the maximal number
		// of supported queues.
		if self.dev_cfg.features.contains(virtio_spec::net::F::MQ) {
			if self.dev_cfg.raw.get_max_virtqueue_pairs() * 2 >= MAX_NUM_VQ {
				self.num_vqs = MAX_NUM_VQ;
			} else {
				self.num_vqs = self.dev_cfg.raw.get_max_virtqueue_pairs() * 2;
			}
		} else {
			// Minimal number of virtqueues defined in the standard v1.1. - 5.1.5 Step 1
			self.num_vqs = 2;
		}

		// The loop is running from 0 to num_vqs and the indexes are provided to the VqIndex::from function in this way
		// in order to allow the indexes of the queues to be in a form of:
		//
		// index i for receiv queue
		// index i+1 for send queue
		//
		// as it is wanted by the network network device.
		// see Virtio specification v1.1. - 5.1.2
		// Assure that we have always an even number of queues (i.e. pairs of queues).
		assert_eq!(self.num_vqs % 2, 0);

		for i in 0..(self.num_vqs / 2) {
			if self
				.dev_cfg
				.features
				.contains(virtio_spec::net::F::RING_PACKED)
			{
				let vq = PackedVq::new(
					&mut self.com_cfg,
					&self.notif_cfg,
					VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
					VqIndex::from(2 * i),
					self.dev_cfg.features.into(),
				)
				.unwrap();
				// Interrupt for receiving packets is wanted
				vq.enable_notifs();

				self.recv_vqs.add(Rc::from(vq), &self.dev_cfg);

				let vq = PackedVq::new(
					&mut self.com_cfg,
					&self.notif_cfg,
					VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
					VqIndex::from(2 * i + 1),
					self.dev_cfg.features.into(),
				)
				.unwrap();
				// Interrupt for comunicating that a sended packet left, is not needed
				vq.disable_notifs();

				self.send_vqs.add(Rc::from(vq), &self.dev_cfg);
			} else {
				let vq = SplitVq::new(
					&mut self.com_cfg,
					&self.notif_cfg,
					VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
					VqIndex::from(2 * i),
					self.dev_cfg.features.into(),
				)
				.unwrap();
				// Interrupt for receiving packets is wanted
				vq.enable_notifs();

				self.recv_vqs.add(Rc::from(vq), &self.dev_cfg);

				let vq = SplitVq::new(
					&mut self.com_cfg,
					&self.notif_cfg,
					VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
					VqIndex::from(2 * i + 1),
					self.dev_cfg.features.into(),
				)
				.unwrap();
				// Interrupt for comunicating that a sended packet left, is not needed
				vq.disable_notifs();

				self.send_vqs.add(Rc::from(vq), &self.dev_cfg);
			}
		}

		Ok(())
	}
}

pub mod constants {
	// Configuration constants
	pub const MAX_NUM_VQ: u16 = 2;

	/// Enum contains virtio's network device status
	/// indiacted in the status field of the device's
	/// configuration structure.
	///
	/// See Virtio specification v1.1. - 5.1.4
	#[allow(dead_code, non_camel_case_types)]
	#[derive(Copy, Clone, Debug)]
	#[repr(u16)]
	pub enum Status {
		VIRTIO_NET_S_LINK_UP = 1 << 0,
		VIRTIO_NET_S_ANNOUNCE = 1 << 1,
	}

	impl From<Status> for u16 {
		fn from(stat: Status) -> Self {
			match stat {
				Status::VIRTIO_NET_S_LINK_UP => 1,
				Status::VIRTIO_NET_S_ANNOUNCE => 2,
			}
		}
	}
}

/// Error module of virtios network driver. Containing the (VirtioNetError)[VirtioNetError]
/// enum.
pub mod error {
	/// Network drivers error enum.
	#[derive(Debug, Copy, Clone)]
	pub enum VirtioNetError {
		#[cfg(feature = "pci")]
		NoDevCfg(u16),
		#[cfg(feature = "pci")]
		NoComCfg(u16),
		#[cfg(feature = "pci")]
		NoIsrCfg(u16),
		#[cfg(feature = "pci")]
		NoNotifCfg(u16),
		FailFeatureNeg(u16),
		/// Set of features does not adhere to the requirements of features
		/// indicated by the specification
		FeatureRequirementsNotMet(virtio_spec::net::F),
		/// The first field contains the feature bits wanted by the driver.
		/// but which are incompatible with the device feature set, second field.
		IncompatibleFeatureSets(virtio_spec::net::F, virtio_spec::net::F),
	}
}
