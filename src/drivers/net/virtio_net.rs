// Copyright (c) 2020 Frederik Schulz, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! A module containing a virtio network driver.
//!
//! The module contains ...
#![allow(unused)]

#[cfg(not(feature = "newlib"))]
use super::netwakeup;
use crate::arch::kernel::pci::error::PciError;
use crate::arch::kernel::pci::PciAdapter;
use crate::arch::kernel::percore::increment_irq_counter;
use crate::config::VIRTIO_MAX_QUEUE_SIZE;
use crate::drivers::net::NetworkInterface;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::convert::TryFrom;
use core::mem;
use core::ops::Deref;
use core::result::Result;

use crate::drivers::virtio::env::memory::{MemLen, MemOff};
use crate::drivers::virtio::error::VirtioError;
use crate::drivers::virtio::transport::pci;
use crate::drivers::virtio::transport::pci::{
	ComCfg, IsrStatus, NotifCfg, NotifCtrl, PciCap, PciCfgAlt, ShMemCfg, UniCapsColl,
};
use crate::drivers::virtio::virtqueue::{
	AsSliceU8, BuffSpec, BufferToken, Bytes, Transfer, TransferToken, Virtq, VqIndex, VqSize,
	VqType,
};

use self::constants::{FeatureSet, Features, NetHdrFlag, NetHdrGSO, Status, MAX_NUM_VQ};
use self::error::VirtioNetError;
use crate::arch::x86_64::mm::paging::{BasePageSize, PageSize};
use crate::arch::x86_64::mm::{paging, virtualmem, VirtAddr};

const ETH_HDR: usize = 14usize;

#[derive(Debug)]
#[repr(C)]
struct VirtioNetHdr {
	flags: u8,
	gso_type: u8,
	/// Ethernet + IP + tcp/udp hdrs
	hdr_len: u16,
	/// Bytes to append to hdr_len per frame
	gso_size: u16,
	/// Position to start checksumming from
	csum_start: u16,
	/// Offset after that to place checksum
	csum_offset: u16,
	/// Number of buffers this Packet consists of
	num_buffers: u16,
}

// Using the default implementation of the trait for VirtioNetHdr
impl AsSliceU8 for VirtioNetHdr {}

impl VirtioNetHdr {
	fn get_tx_hdr() -> VirtioNetHdr {
		VirtioNetHdr {
			flags: 0,
			gso_type: NetHdrGSO::VIRTIO_NET_HDR_GSO_NONE.into(),
			hdr_len: 0,
			gso_size: 0,
			csum_start: 0,
			csum_offset: 0,
			num_buffers: 0,
		}
	}

	fn get_rx_hdr() -> VirtioNetHdr {
		VirtioNetHdr {
			flags: 0,
			gso_type: 0,
			hdr_len: 0,
			gso_size: 0,
			csum_start: 0,
			csum_offset: 0,
			num_buffers: 0,
		}
	}
}

/// A wrapper struct for the raw configuration structure.
/// Handling the right access to fields, as some are read-only
/// for the driver.
struct NetDevCfg {
	raw: &'static NetDevCfgRaw,
	dev_id: u16,

	// Feature booleans
	features: FeatureSet,
}

/// Virtio's network device configuration structure.
/// See specification v1.1. - 5.1.4
///
#[repr(C)]
struct NetDevCfgRaw {
	// Specifies Mac address, only Valid if VIRTIO_NET_F_MAC is set
	mac: [u8; 6],
	// Indicates status of device. Only valid if VIRTIO_NET_F_STATUS is set
	status: u16,
	// Indicates number of allowed vq-pairs. Only valid if VIRTIO_NET_F_MQ is set.
	max_virtqueue_pairs: u16,
	// Indicates the maximum MTU driver should use. Only valid if VIRTIONET_F_MTU is set.
	mtu: u16,
}

struct CtrlQueue(Option<Rc<Virtq>>);

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

struct RxQueues {
	vqs: Vec<Rc<Virtq>>,
	poll_queue: Rc<RefCell<VecDeque<Transfer>>>,
	is_multi: bool,
}

impl RxQueues {
	/// Takes care if handling packets correctly which need some processing after beeing received.
	/// This currently include nothing. But in the future it might include among others::
	/// * Calculating missing checksums
	/// * Merging receive buffers, by simply checking the poll_queue (if VIRTIO_NET_F_MRG_BUF)
	fn post_processing(mut transfer: Transfer) -> Result<Transfer, VirtioNetError> {
		if transfer.poll() {
			// Here we could implement all features.
			Ok(transfer)
		} else {
			warn!("Unfinished transfer in post processing. Returning buffer to queue. This will need explicit cleanup.");
			transfer.close();
			Err(VirtioNetError::ProcessOngoing)
		}
	}

	/// Adds a given queue to the underlying vector and populates the queue with RecvBuffers.
	///
	/// Queues are all populated according to Virtio specification v1.1. - 5.1.6.3.1
	fn add(&mut self, vq: Virtq, dev_cfg: &NetDevCfg) {
		// Safe virtqueue
		let rc_vq = Rc::new(vq);
		let vq = &rc_vq;

		if dev_cfg
			.features
			.is_feature(Features::VIRTIO_NET_F_GUEST_TSO4)
			| dev_cfg
				.features
				.is_feature(Features::VIRTIO_NET_F_GUEST_TSO6)
			| dev_cfg
				.features
				.is_feature(Features::VIRTIO_NET_F_GUEST_UFO)
		{
			// Receive Buffers must be at least 65562 bytes large with theses features set.
			// See Virtio specification v1.1 - 5.1.6.3.1

			// Currently we choose indirect descriptors if posible in order to allow
			// as many packages as possible inside the queue.
			let buff_def = [
				Bytes::new(mem::size_of::<VirtioNetHdr>()).unwrap(),
				Bytes::new(65550).unwrap(),
			];

			let spec = if dev_cfg
				.features
				.is_feature(Features::VIRTIO_F_RING_INDIRECT_DESC)
			{
				BuffSpec::Indirect(&buff_def)
			} else {
				BuffSpec::Single(Bytes::new(mem::size_of::<VirtioNetHdr>() + 65550).unwrap())
			};

			let num_buff: u16 = if dev_cfg
				.features
				.is_feature(Features::VIRTIO_F_RING_INDIRECT_DESC)
			{
				vq.size().into()
			} else {
				vq.size().into()
			};

			for _ in 0..num_buff {
				let buff_tkn = match vq.prep_buffer(Rc::clone(vq), None, Some(spec.clone())) {
					Ok(tkn) => tkn,
					Err(vq_err) => {
						error!("Setup of network queue failed, which should not happen!");
						panic!("setup of network queue failed!");
					}
				};

				// BufferTokens are directly provided to the queue
				// TransferTokens are directly dispatched
				// Transfers will be awaited at the queue
				buff_tkn
					.provide()
					.dispatch_await(Rc::clone(&self.poll_queue), false);
			}
		} else {
			// If above features not set, buffers must be at least 1526 bytes large.
			// See Virtio specification v1.1 - 5.1.6.3.1
			//
			let buff_def = [
				Bytes::new(mem::size_of::<VirtioNetHdr>()).unwrap(),
				Bytes::new(1514).unwrap(),
			];
			let spec = if dev_cfg
				.features
				.is_feature(Features::VIRTIO_F_RING_INDIRECT_DESC)
			{
				BuffSpec::Indirect(&buff_def)
			} else {
				BuffSpec::Single(Bytes::new(mem::size_of::<VirtioNetHdr>() + 1514).unwrap())
			};

			let num_buff: u16 = if dev_cfg
				.features
				.is_feature(Features::VIRTIO_F_RING_INDIRECT_DESC)
			{
				vq.size().into()
			} else {
				vq.size().into()
			};

			for _ in 0..num_buff {
				let buff_tkn = match vq.prep_buffer(Rc::clone(vq), None, Some(spec.clone())) {
					Ok(tkn) => tkn,
					Err(vq_err) => {
						error!("Setup of network queue failed, which should not happen!");
						panic!("setup of network queue failed!");
					}
				};

				// BufferTokens are directly provided to the queue
				// TransferTokens are directly dispatched
				// Transfers will be awaited at the queue
				buff_tkn
					.provide()
					.dispatch_await(Rc::clone(&self.poll_queue), false);
			}
		}

		// Safe virtqueue
		self.vqs.push(rc_vq);

		if self.vqs.len() > 1 {
			self.is_multi = true;
		}
	}

	fn get_next(&mut self) -> Option<Transfer> {
		let transfer = match self.poll_queue.borrow_mut().pop_front() {
			Some(transfer) => Some(transfer),
			None => None,
		};

		match transfer {
			Some(transfer) => Some(transfer),
			None => {
				// Check if any not yet provided transfers are in the queue.
				self.poll();

				match self.poll_queue.borrow_mut().pop_front() {
					Some(transfer) => Some(transfer),
					None => None,
				}
			}
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
struct TxQueues {
	vqs: Vec<Rc<Virtq>>,
	poll_queue: Rc<RefCell<VecDeque<Transfer>>>,
	ready_queue: Vec<BufferToken>,
	/// Indicates, whether the Driver/Device are using multiple
	/// queues for communication.
	is_multi: bool,
}

impl TxQueues {
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

	fn poll(&self) {
		if self.is_multi {
			for vq in &self.vqs {
				vq.poll();
			}
		} else {
			self.vqs[0].poll();
		}
	}

	fn add(&mut self, vq: Virtq, dev_cfg: &NetDevCfg) {
		// Safe virtqueue
		self.vqs.push(Rc::new(vq));
		if self.vqs.len() == 1 {
			// Unwrapping is safe, as one virtq will be definitely in the vector.
			let vq = self.vqs.get(0).unwrap();

			// Virtio specification v1.1. - 5.1.6.2 point 5.
			//      Header and data are added as ONE output descriptor to the transmitvq.
			//      Hence we are interpreting this, as the fact, that send packets must be inside a single descriptor.
			// As usize is currently safe as the minimal usize is defined as 16bit in rust.
			let buff_def =
				Bytes::new(mem::size_of::<VirtioNetHdr>() + (dev_cfg.raw.mtu as usize) + ETH_HDR)
					.unwrap();
			let spec = BuffSpec::Single(buff_def);

			let num_buff: u16 = vq.size().into();

			for _ in 0..num_buff {
				self.ready_queue.push(
					vq.prep_buffer(Rc::clone(vq), Some(spec.clone()), None)
						.unwrap()
						.write_seq(Some(VirtioNetHdr::get_tx_hdr()), None::<VirtioNetHdr>)
						.unwrap(),
				)
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
		// All Tokens inside the ready_queue are comming from the main queu with index 0.
		while let Some(mut tkn) = self.ready_queue.pop() {
			let (send_len, _) = tkn.len();

			if send_len == len {
				return Some((tkn, 0));
			} else if send_len > len {
				tkn.restr_size(Some(len), None).unwrap();
				return Some((tkn, 0));
			} else {
				// Otherwise we are freeing the queue from the token.
				drop(tkn);
			}
		}

		if self.poll_queue.borrow().is_empty() {
			self.poll();
		}

		while let Some(transfer) = self.poll_queue.borrow_mut().pop_back() {
			let mut tkn = transfer.reuse().unwrap();
			let (send_len, _) = tkn.len();

			if send_len == len {
				return Some((tkn, 0));
			} else if send_len > len {
				tkn.restr_size(Some(len), None).unwrap();
				return Some((tkn, 0));
			} else {
				// Otherwise we are freeing the queue from the token.
				drop(tkn);
			}
		}

		// As usize is currently safe as the minimal usize is defined as 16bit in rust.
		let spec = BuffSpec::Single(Bytes::new(len).unwrap());

		match self.vqs[0].prep_buffer(Rc::clone(&self.vqs[0]), Some(spec), None) {
			Ok(tkn) => return Some((tkn, 0)),
			Err(_) => {
				// Here it is possible if multiple ques are enabled to get another buffertoken from them!
				// Info the queues are disbaled upon initialization and should be enabled somehow!
				None
			}
		}
	}
}

/// Virtio network driver struct.
///
/// Struct allows to control devices virtqueues as also
/// the device itself.
pub struct VirtioNetDriver {
	dev_cfg: NetDevCfg,
	com_cfg: ComCfg,
	isr_stat: IsrStatus,
	notif_cfg: NotifCfg,

	ctrl_vq: CtrlQueue,
	recv_vqs: RxQueues,
	send_vqs: TxQueues,

	num_vqs: u16,
	irq: u8,
}

impl NetworkInterface for VirtioNetDriver {
	/// Returns the mac address of the device.
	/// If VIRTIO_NET_F_MAC is not set, the function panics currently!
	fn get_mac_address(&self) -> [u8; 6] {
		if self.dev_cfg.features.is_feature(Features::VIRTIO_NET_F_MAC) {
			self.dev_cfg.raw.mac
		} else {
			unreachable!("Currently VIRTIO_NET_F_MAC must be negotiated!")
		}
	}

	/// Returns the current MTU of the device.
	/// Currently, if VIRTIO_NET_F_MAC is not set
	//  MTU is set static to 1500 bytes.
	fn get_mtu(&self) -> u16 {
		if self.dev_cfg.features.is_feature(Features::VIRTIO_NET_F_MTU) {
			self.dev_cfg.raw.mtu
		} else {
			1500
		}
	}

	/// Provides the "user-space" with a pointer to usable memory.
	///
	/// Therefore the driver checks if a free BufferToken is in its TxQueues struct.
	/// If one is found, the function does return a pointer to the memory area, where
	/// the "user-space" can write to and a raw pointer to the token in order to provide
	/// it to the queue after the "user-space" driver has written to the buffer.
	///
	/// If not BufferToken is found the functions returns an error.
	fn get_tx_buffer(&mut self, len: usize) -> Result<(*mut u8, usize), ()> {
		// Adding virtio header size to length.
		let len = len + core::mem::size_of::<VirtioNetHdr>();

		match self.send_vqs.get_tkn(len) {
			Some((mut buff_tkn, vq_index)) => {
				let (send_ptrs, _) = buff_tkn.raw_ptrs();
				// Currently we have single Buffers in the TxQueue of size: MTU + ETH_HDR + VIRTIO_NET_HDR
				// see TxQueue.add()
				let (buff_ptr, _) = send_ptrs.unwrap()[0];

				// Do not show user-space memory for VirtioNetHdr.
				let buff_ptr = unsafe {
					buff_ptr.offset(isize::try_from(core::mem::size_of::<VirtioNetHdr>()).unwrap())
				};

				Ok((buff_ptr, Box::into_raw(Box::new(buff_tkn)) as usize))
			}
			None => Err(()),
		}
	}

	fn send_tx_buffer(&mut self, tkn_handle: usize, len: usize) -> Result<(), ()> {
		// This does not result in a new assignment, or in a drop of the BufferToken, which
		// would be dangerous, as the memory is freed then.
		let tkn = *unsafe { Box::from_raw(tkn_handle as *mut BufferToken) };

		tkn.provide()
			.dispatch_await(Rc::clone(&self.send_vqs.poll_queue), false);

		Ok(())
	}

	fn has_packet(&self) -> bool {
		self.recv_vqs.poll();
		!self.recv_vqs.poll_queue.borrow().is_empty()
	}

	fn receive_rx_buffer(&mut self) -> Result<(&'static [u8], usize), ()> {
		match self.recv_vqs.get_next() {
			Some(mut transfer) => {
				let mut transfer = match RxQueues::post_processing(transfer) {
					Ok(trf) => trf,
					Err(vnet_err) => {
						error!("Post processing failed. Err: {:?}", vnet_err);
						return Err(());
					}
				};

				let (_, recv_data_opt) = transfer.as_slices().unwrap();
				let mut recv_data = recv_data_opt.unwrap();

				// If the given length is zero, we currently fail.
				if recv_data.len() == 2 {
					let recv_payload = recv_data.pop().unwrap();
					// Create static refrence for the user-space
					// As long as we keep the Transfer in a raw refernce this refernce is static,
					// so this is fine.
					let recv_ref = (recv_payload as *const [u8]) as *mut [u8];
					let ref_data: &'static [u8] = unsafe { &*(recv_ref) };
					let raw_transfer = Box::into_raw(Box::new(transfer));

					Ok((ref_data, raw_transfer as usize))
				} else if recv_data.len() == 1 {
					let packet = recv_data.pop().unwrap();
					let payload_ptr =
						(&packet[mem::size_of::<VirtioNetHdr>()] as *const u8) as *mut u8;

					let ref_data: &'static [u8] = unsafe {
						core::slice::from_raw_parts(
							payload_ptr,
							packet.len() - mem::size_of::<VirtioNetHdr>(),
						)
					};
					let raw_transfer = Box::into_raw(Box::new(transfer));

					Ok((ref_data, raw_transfer as usize))
				} else {
					error!("Empty transfer, or with wrong buffer layout. Reusing and returning error to user-space network driver...");
					transfer
						.reuse()
						.unwrap()
						.write_seq(None::<VirtioNetHdr>, Some(VirtioNetHdr::get_rx_hdr()))
						.unwrap()
						.provide()
						.dispatch_await(Rc::clone(&self.recv_vqs.poll_queue), false);

					Err(())
				}
			}
			None => Err(()),
		}
	}

	// Tells driver, that buffer is consumed and can be deallocated
	fn rx_buffer_consumed(&mut self, trf_handle: usize) {
		unsafe {
			let transfer = *Box::from_raw(trf_handle as *mut Transfer);

			// Reuse transfer directly
			transfer
				.reuse()
				.unwrap()
				.provide()
				.dispatch_await(Rc::clone(&self.recv_vqs.poll_queue), false);
		}
	}

	fn set_polling_mode(&mut self, value: bool) {
		if value {
			self.disable_interrupts()
		} else {
			self.enable_interrupts()
		}
	}

	fn handle_interrupt(&mut self) -> bool {
		increment_irq_counter((32 + self.irq).into());

		if self.isr_stat.is_interrupt() {
			// handle incoming packets
			#[cfg(not(feature = "newlib"))]
			netwakeup();

			true
		} else if self.isr_stat.is_cfg_change() {
			info!("Configuration changes are not possible! Aborting");
			todo!("Implement possibiity to change config on the fly...");
			false
		} else {
			false
		}
	}
}

// Kernel interface
impl VirtioNetDriver {
	/// Returns the current status of the device, if VIRTIO_NET_F_STATUS
	/// has been negotiated. Otherwise returns zero.
	pub fn dev_status(&self) -> u16 {
		if self
			.dev_cfg
			.features
			.is_feature(Features::VIRTIO_NET_F_STATUS)
		{
			self.dev_cfg.raw.status
		} else {
			0
		}
	}

	/// Returns the links status.
	/// If feature VIRTIO_NET_F_STATUS has not been negotiated, then we assume the link is up!
	fn is_link_up(&self) -> bool {
		if self
			.dev_cfg
			.features
			.is_feature(Features::VIRTIO_NET_F_STATUS)
		{
			self.dev_cfg.raw.status & u16::from(Status::VIRTIO_NET_S_LINK_UP)
				== u16::from(Status::VIRTIO_NET_S_LINK_UP)
		} else {
			true
		}
	}

	fn is_announce(&self) -> bool {
		if self
			.dev_cfg
			.features
			.is_feature(Features::VIRTIO_NET_F_STATUS)
		{
			self.dev_cfg.raw.status & u16::from(Status::VIRTIO_NET_S_ANNOUNCE)
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
	fn get_max_vq_pairs(&self) -> u16 {
		if self.dev_cfg.features.is_feature(Features::VIRTIO_NET_F_MQ) {
			self.dev_cfg.raw.max_virtqueue_pairs
		} else {
			1
		}
	}

	pub fn disable_interrupts(&self) {
		// Für send und receive queues?
		// Nur für receive? Weil send eh ausgeschaltet ist?
		self.recv_vqs.disable_notifs();
	}

	pub fn enable_interrupts(&self) {
		// Für send und receive queues?
		// Nur für receive? Weil send eh ausgeschaltet ist?
		self.recv_vqs.enable_notifs();
	}
}

// Private funtctions for Virtio network driver
impl VirtioNetDriver {
	fn map_cfg(cap: &PciCap) -> Option<NetDevCfg> {
		/*
		if cap.bar_len() <  u64::from(cap.len() + cap.offset()) {
			error!("Network config of device {:x}, does not fit into memeory specified by bar!",
				cap.dev_id(),
			);
			return None
		}

		// Drivers MAY do this check. See Virtio specification v1.1. - 4.1.4.1
		if cap.len() < MemLen::from(mem::size_of::<NetDevCfg>()*8) {
			error!("Network config from device {:x}, does not represent actual structure specified by the standard!", cap.dev_id());
			return None
		}

		let virt_addr_raw = cap.bar_addr() + cap.offset();

		// Create mutable reference to the PCI structure in PCI memory
		let dev_cfg: &mut NetDevCfgRaw = unsafe {
			&mut *(usize::from(virt_addr_raw) as *mut NetDevCfgRaw)
		};
		*/
		let dev_cfg: &'static NetDevCfgRaw = match pci::map_dev_cfg::<NetDevCfgRaw>(cap) {
			Some(cfg) => cfg,
			None => return None,
		};

		Some(NetDevCfg {
			raw: dev_cfg,
			dev_id: cap.dev_id(),
			features: FeatureSet::new(0),
		})
	}

	/// Instanciates a new (VirtioNetDriver)[VirtioNetDriver] struct, by checking the available
	/// configuration structures and moving them into the struct.
	fn new(
		mut caps_coll: UniCapsColl,
		adapter: &PciAdapter,
	) -> Result<Self, error::VirtioNetError> {
		let com_cfg = loop {
			match caps_coll.get_com_cfg() {
				Some(com_cfg) => break com_cfg,
				None => {
					error!("No common config. Aborting!");
					return Err(error::VirtioNetError::NoComCfg(adapter.device_id));
				}
			}
		};

		let isr_stat = loop {
			match caps_coll.get_isr_cfg() {
				Some(isr_stat) => break isr_stat,
				None => {
					error!("No ISR status config. Aborting!");
					return Err(error::VirtioNetError::NoIsrCfg(adapter.device_id));
				}
			}
		};

		let notif_cfg = loop {
			match caps_coll.get_notif_cfg() {
				Some(notif_cfg) => break notif_cfg,
				None => {
					error!("No notif config. Aborting!");
					return Err(error::VirtioNetError::NoNotifCfg(adapter.device_id));
				}
			}
		};

		let dev_cfg = loop {
			match caps_coll.get_dev_cfg() {
				Some(cfg) => match VirtioNetDriver::map_cfg(&cfg) {
					Some(dev_cfg) => break dev_cfg,
					None => (),
				},
				None => {
					error!("No dev config. Aborting!");
					return Err(error::VirtioNetError::NoDevCfg(adapter.device_id));
				}
			}
		};

		Ok(VirtioNetDriver {
			dev_cfg,
			com_cfg,
			isr_stat,
			notif_cfg,

			ctrl_vq: CtrlQueue(None),
			recv_vqs: RxQueues {
				vqs: Vec::<Rc<Virtq>>::new(),
				poll_queue: Rc::new(RefCell::new(VecDeque::new())),
				is_multi: false,
			},
			send_vqs: TxQueues {
				vqs: Vec::<Rc<Virtq>>::new(),
				poll_queue: Rc::new(RefCell::new(VecDeque::new())),
				ready_queue: Vec::new(),
				is_multi: false,
			},
			num_vqs: 0,
			irq: adapter.irq,
		})
	}

	/// Initiallizes the device in adherence to specificaton. Returns Some(VirtioNetError)
	/// upon failure and None in case everything worked as expected.
	///
	/// See Virtio specification v1.1. - 3.1.1.
	///                      and v1.1. - 5.1.5
	fn init_dev(&mut self) -> Result<(), VirtioNetError> {
		// Reset
		self.com_cfg.reset_dev();

		// Indiacte device, that OS noticed it
		self.com_cfg.ack_dev();

		// Indicate device, that driver is able to handle it
		self.com_cfg.set_drv();

		// Define minimal feature set
		let min_feats: Vec<Features> = vec![
			Features::VIRTIO_F_VERSION_1,
			Features::VIRTIO_NET_F_MAC,
			Features::VIRTIO_NET_F_STATUS,
		];

		let mut min_feat_set = FeatureSet::new(0);
		min_feat_set.set_features(&min_feats);
		let mut feats: Vec<Features> = Vec::from(min_feats);

		// If wanted, push new features into feats here:
		//
		// Indirect descriptors can be used
		feats.push(Features::VIRTIO_F_RING_INDIRECT_DESC);
		// MTU setting can be used
		feats.push(Features::VIRTIO_NET_F_MTU);
		// Packed Vq can be used
		feats.push(Features::VIRTIO_F_RING_PACKED);

		// Currently the driver does NOT support the features below.
		// In order to provide functionality for theses, the driver
		// needs to take care of calculating checksum in
		// RxQueues.post_processing()
		// feats.push(Features::VIRTIO_NET_F_GUEST_CSUM);
		// feats.push(Features::VIRTIO_NET_F_GUEST_TSO4);
		// feats.push(Features::VIRTIO_NET_F_GUEST_TSO6);

		// Negotiate features with device. Automatically reduces selected feats in order to meet device capabilites.
		// Aborts in case incompatible features are selected by the dricer or the device does not support min_feat_set.
		match self.negotiate_features(&feats) {
			Ok(_) => info!(
				"Driver found a subset of features for virtio device {:x}. Features are: {:?}",
				self.dev_cfg.dev_id, &feats
			),
			Err(vnet_err) => {
				match vnet_err {
					VirtioNetError::FeatReqNotMet(feat_set) => {
						error!("Network drivers feature set {:x} does not satisfy rules in section 5.1.3.1 of specification v1.1. Aborting!", u64::from(feat_set));
						return Err(vnet_err);
					}
					VirtioNetError::IncompFeatsSet(drv_feats, dev_feats) => {
						// Create a new matching feature set for device and driver if the minimal set is met!
						if (min_feat_set & dev_feats) != min_feat_set {
							error!("Device features set, does not satisfy minimal features needed. Aborting!");
							return Err(VirtioNetError::FailFeatureNeg(self.dev_cfg.dev_id));
						} else {
							feats = match Features::into_features(dev_feats & drv_feats) {
								Some(feats) => feats,
								None => {
									error!("Feature negotiation failed with minimal feature set. Aborting!");
									return Err(VirtioNetError::FailFeatureNeg(
										self.dev_cfg.dev_id,
									));
								}
							};

							match self.negotiate_features(&feats) {
                                Ok(_) => info!("Driver found a subset of features for virtio device {:x}. Features are: {:?}", self.dev_cfg.dev_id, &feats),
                                Err(vnet_err) => {
                                    match vnet_err {
                                        VirtioNetError::FeatReqNotMet(feat_set) => {
                                            error!("Network device offers a feature set {:x} when used completly does not satisfy rules in section 5.1.3.1 of specification v1.1. Aborting!", u64::from(feat_set));
                                            return Err(vnet_err);
                                        },
                                        _ => {
                                            error!("Feature Set after reduction still not usable. Set: {:?}. Aborting!", feats);
                                            return Err(vnet_err);
                                        }
                                    }
                                }
                            }
						}
					}
					_ => {
						error!(
							"Wanted set of features is NOT supported by device. Set: {:?}",
							feats
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
			self.dev_cfg.features.set_features(&feats);
		} else {
			return Err(VirtioNetError::FailFeatureNeg(self.dev_cfg.dev_id));
		}

		match self.dev_spec_init() {
			Ok(_) => info!(
				"Device specific initialization for Virtio network defice {:x} finished",
				self.dev_cfg.dev_id
			),
			Err(vnet_err) => return Err(vnet_err),
		}
		// At this point the device is "live"
		self.com_cfg.drv_ok();

		Ok(())
	}

	/// Negotiates a subset of features, understood and wanted by both the OS
	/// and the device.
	fn negotiate_features(&mut self, wanted_feats: &Vec<Features>) -> Result<(), VirtioNetError> {
		let mut drv_feats = FeatureSet::new(0);

		for feat in wanted_feats.iter() {
			drv_feats |= *feat;
		}

		let dev_feats = FeatureSet::new(self.com_cfg.dev_features());

		// Checks if the selected feature set is compatible with requirements for
		// features according to Virtio spec. v1.1 - 5.1.3.1.
		match FeatureSet::check_features(&wanted_feats) {
			Ok(_) => {
				info!("Feature set wanted by network driver are in conformance with specification.")
			}
			Err(vnet_err) => return Err(vnet_err),
		}

		if (dev_feats & drv_feats) == drv_feats {
			// If device supports subset of features write feature set to common config
			self.com_cfg.set_drv_features(drv_feats.into());
			Ok(())
		} else {
			Err(VirtioNetError::IncompFeatsSet(drv_feats, dev_feats))
		}
	}

	/// Device Specfic initialization according to Virtio specifictation v1.1. - 5.1.5
	fn dev_spec_init(&mut self) -> Result<(), VirtioNetError> {
		match self.virtqueue_init() {
			Ok(_) => info!("Network driver successfully initialized virtqueues."),
			Err(vnet_err) => return Err(vnet_err),
		}

		// Add a control if feature is negotiated
		if self
			.dev_cfg
			.features
			.is_feature(Features::VIRTIO_NET_F_CTRL_VQ)
		{
			if self
				.dev_cfg
				.features
				.is_feature(Features::VIRTIO_F_RING_PACKED)
			{
				self.ctrl_vq = CtrlQueue(Some(Rc::new(Virtq::new(
					&mut self.com_cfg,
					&self.notif_cfg,
					VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
					VqType::Packed,
					VqIndex::from(self.num_vqs),
					self.dev_cfg.features.into(),
				))));

				self.ctrl_vq.0.as_ref().unwrap().enable_notifs();
			} else {
				self.ctrl_vq = CtrlQueue(Some(Rc::new(Virtq::new(
					&mut self.com_cfg,
					&self.notif_cfg,
					VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
					VqType::Split,
					VqIndex::from(self.num_vqs),
					self.dev_cfg.features.into(),
				))));

				self.ctrl_vq.0.as_ref().unwrap().enable_notifs();
			}
		}

		// If device does not take care of MAC address, the driver has to create one
		if !self.dev_cfg.features.is_feature(Features::VIRTIO_NET_F_MAC) {
			todo!("Driver created MAC address should be passed to device here.")
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
		// - the plus 1 is due to the possibility of an exisiting control queue
		// - the num_queues is found in the ComCfg struct of the device and defines the maximal number
		// of supported queues.
		if self.dev_cfg.features.is_feature(Features::VIRTIO_NET_F_MQ) {
			if self.dev_cfg.raw.max_virtqueue_pairs * 2 >= MAX_NUM_VQ {
				self.num_vqs = MAX_NUM_VQ;
			} else {
				self.num_vqs = self.dev_cfg.raw.max_virtqueue_pairs * 2;
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
				.is_feature(Features::VIRTIO_F_RING_PACKED)
			{
				let vq = Virtq::new(
					&mut self.com_cfg,
					&self.notif_cfg,
					VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
					VqType::Packed,
					VqIndex::from(2 * i),
					self.dev_cfg.features.into(),
				);
				// Interrupt for receiving packets is wanted
				vq.enable_notifs();

				self.recv_vqs.add(vq, &self.dev_cfg);

				let vq = Virtq::new(
					&mut self.com_cfg,
					&self.notif_cfg,
					VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
					VqType::Packed,
					VqIndex::from(2 * i + 1),
					self.dev_cfg.features.into(),
				);
				// Interrupt for comunicating that a sended packet left, is not needed
				vq.disable_notifs();

				self.send_vqs.add(vq, &self.dev_cfg);
			} else {
				let vq = Virtq::new(
					&mut self.com_cfg,
					&self.notif_cfg,
					VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
					VqType::Split,
					VqIndex::from(2 * i),
					self.dev_cfg.features.into(),
				);
				// Interrupt for receiving packets is wanted
				vq.enable_notifs();

				self.recv_vqs.add(vq, &self.dev_cfg);

				let vq = Virtq::new(
					&mut self.com_cfg,
					&self.notif_cfg,
					VqSize::from(VIRTIO_MAX_QUEUE_SIZE),
					VqType::Split,
					VqIndex::from(2 * i + 1),
					self.dev_cfg.features.into(),
				);
				// Interrupt for comunicating that a sended packet left, is not needed
				vq.disable_notifs();

				self.send_vqs.add(vq, &self.dev_cfg);
			}
		}
		Ok(())
	}
}

// Public interface for virtio network driver.
impl VirtioNetDriver {
	/// Initializes virtio network device by mapping configuration layout to
	/// respective structs (configuration structs are:
	/// [ComCfg](structs.comcfg.html), [NotifCfg](structs.notifcfg.html)
	/// [IsrStatus](structs.isrstatus.html), [PciCfg](structs.pcicfg.html)
	/// [ShMemCfg](structs.ShMemCfg)).
	///
	/// Returns a driver instance of
	/// [VirtioNetDriver](structs.virtionetdriver.html) or an [VirtioError](enums.virtioerror.html).
	pub fn init(adapter: &PciAdapter) -> Result<VirtioNetDriver, VirtioError> {
		let mut drv = match pci::map_caps(adapter) {
			Ok(caps) => match VirtioNetDriver::new(caps, adapter) {
				Ok(driver) => driver,
				Err(vnet_err) => {
					error!("Initializing new network driver failed. Aborting!");
					return Err(VirtioError::NetDriver(vnet_err));
				}
			},
			Err(pci_error) => {
				error!("Mapping capabilites failed. Aborting!");
				return Err(VirtioError::FromPci(pci_error));
			}
		};

		match drv.init_dev() {
			Ok(_) => info!(
				"Network device with id {:x}, has been initialized by driver!",
				drv.dev_cfg.dev_id
			),
			Err(vnet_err) => {
				drv.com_cfg.set_failed();
				return Err(VirtioError::NetDriver(vnet_err));
			}
		}

		if drv.is_link_up() {
			info!("Virtio-net link is up after initialization.")
		} else {
			info!("Virtio-net link is down after initialization!")
		}

		Ok(drv)
	}
}

mod constants {
	use super::error::VirtioNetError;
	use alloc::vec::Vec;
	use core::fmt::Display;
	use core::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign};

	// Configuration constants
	pub const MAX_NUM_VQ: u16 = 2;

	/// Enum containing Virtios netword header flags
	///
	/// See Virtio specification v1.1. - 5.1.6
	#[allow(dead_code, non_camel_case_types)]
	#[derive(Copy, Clone, Debug)]
	#[repr(u8)]
	///
	pub enum NetHdrFlag {
		/// use csum_start, csum_offset
		VIRTIO_NET_HDR_F_NEEDS_CSUM = 1,
		/// csum is valid
		VIRTIO_NET_HDR_F_DATA_VALID = 2,
		/// reports number of coalesced TCP segments
		VIRTIO_NET_HDR_F_RSC_INFO = 4,
	}

	impl From<NetHdrFlag> for u8 {
		fn from(val: NetHdrFlag) -> Self {
			match val {
				NetHdrFlag::VIRTIO_NET_HDR_F_NEEDS_CSUM => 1,
				NetHdrFlag::VIRTIO_NET_HDR_F_DATA_VALID => 2,
				NetHdrFlag::VIRTIO_NET_HDR_F_RSC_INFO => 4,
			}
		}
	}

	impl BitOr for NetHdrFlag {
		type Output = u8;

		fn bitor(self, rhs: Self) -> Self::Output {
			u8::from(self) | u8::from(rhs)
		}
	}

	impl BitOr<NetHdrFlag> for u8 {
		type Output = u8;

		fn bitor(self, rhs: NetHdrFlag) -> Self::Output {
			self | u8::from(rhs)
		}
	}

	impl BitOrAssign<NetHdrFlag> for u8 {
		fn bitor_assign(&mut self, rhs: NetHdrFlag) {
			*self |= u8::from(rhs);
		}
	}

	impl BitAnd for NetHdrFlag {
		type Output = u8;

		fn bitand(self, rhs: NetHdrFlag) -> Self::Output {
			u8::from(self) & u8::from(rhs)
		}
	}

	impl BitAnd<NetHdrFlag> for u8 {
		type Output = u8;

		fn bitand(self, rhs: NetHdrFlag) -> Self::Output {
			self & u8::from(rhs)
		}
	}

	impl BitAndAssign<NetHdrFlag> for u8 {
		fn bitand_assign(&mut self, rhs: NetHdrFlag) {
			*self &= u8::from(rhs);
		}
	}

	/// Enum containing Virtios netword GSO types
	///
	/// See Virtio specification v1.1. - 5.1.6
	#[allow(dead_code, non_camel_case_types)]
	#[derive(Copy, Clone, Debug)]
	#[repr(u8)]
	pub enum NetHdrGSO {
		/// not a GSO frame
		VIRTIO_NET_HDR_GSO_NONE = 0,
		/// GSO frame, IPv4 TCP (TSO)
		VIRTIO_NET_HDR_GSO_TCPV4 = 1,
		/// GSO frame, IPv4 UDP (UFO)
		VIRTIO_NET_HDR_GSO_UDP = 3,
		/// GSO frame, IPv6 TCP
		VIRTIO_NET_HDR_GSO_TCPV6 = 4,
		/// TCP has ECN set
		VIRTIO_NET_HDR_GSO_ECN = 0x80,
	}

	impl From<NetHdrGSO> for u8 {
		fn from(val: NetHdrGSO) -> Self {
			match val {
				NetHdrGSO::VIRTIO_NET_HDR_GSO_NONE => 0,
				NetHdrGSO::VIRTIO_NET_HDR_GSO_TCPV4 => 1,
				NetHdrGSO::VIRTIO_NET_HDR_GSO_UDP => 3,
				NetHdrGSO::VIRTIO_NET_HDR_GSO_TCPV6 => 4,
				NetHdrGSO::VIRTIO_NET_HDR_GSO_ECN => 0x80,
			}
		}
	}

	impl BitOr for NetHdrGSO {
		type Output = u8;

		fn bitor(self, rhs: Self) -> Self::Output {
			u8::from(self) | u8::from(rhs)
		}
	}

	impl BitOr<NetHdrGSO> for u8 {
		type Output = u8;

		fn bitor(self, rhs: NetHdrGSO) -> Self::Output {
			self | u8::from(rhs)
		}
	}

	impl BitOrAssign<NetHdrGSO> for u8 {
		fn bitor_assign(&mut self, rhs: NetHdrGSO) {
			*self |= u8::from(rhs);
		}
	}

	impl BitAnd for NetHdrGSO {
		type Output = u8;

		fn bitand(self, rhs: NetHdrGSO) -> Self::Output {
			u8::from(self) & u8::from(rhs)
		}
	}

	impl BitAnd<NetHdrGSO> for u8 {
		type Output = u8;

		fn bitand(self, rhs: NetHdrGSO) -> Self::Output {
			self & u8::from(rhs)
		}
	}

	impl BitAndAssign<NetHdrGSO> for u8 {
		fn bitand_assign(&mut self, rhs: NetHdrGSO) {
			*self &= u8::from(rhs);
		}
	}

	/// Enum contains virtio's network device features and general features of Virtio.
	///
	/// See Virtio specification v1.1. - 5.1.3
	///
	/// See Virtio specification v1.1. - 6
	//
	// WARN: In case the enum is changed, the static function of features `into_features(feat: u64) ->
	// Option<Vec<Features>>` must also be adjusted to return a corret vector of features.
	#[allow(dead_code, non_camel_case_types)]
	#[derive(Copy, Clone, Debug)]
	#[repr(u64)]
	pub enum Features {
		VIRTIO_NET_F_CSUM = 1 << 0,
		VIRTIO_NET_F_GUEST_CSUM = 1 << 1,
		VIRTIO_NET_F_CTRL_GUEST_OFFLOADS = 1 << 2,
		VIRTIO_NET_F_MTU = 1 << 3,
		VIRTIO_NET_F_MAC = 1 << 5,
		VIRTIO_NET_F_GUEST_TSO4 = 1 << 7,
		VIRTIO_NET_F_GUEST_TSO6 = 1 << 8,
		VIRTIO_NET_F_GUEST_ECN = 1 << 9,
		VIRTIO_NET_F_GUEST_UFO = 1 << 10,
		VIRTIO_NET_F_HOST_TSO4 = 1 << 11,
		VIRTIO_NET_F_HOST_TSO6 = 1 << 12,
		VIRTIO_NET_F_HOST_ECN = 1 << 13,
		VIRTIO_NET_F_HOST_UFO = 1 << 14,
		VIRTIO_NET_F_MRG_RXBUF = 1 << 15,
		VIRTIO_NET_F_STATUS = 1 << 16,
		VIRTIO_NET_F_CTRL_VQ = 1 << 17,
		VIRTIO_NET_F_CTRL_RX = 1 << 18,
		VIRTIO_NET_F_CTRL_VLAN = 1 << 19,
		VIRTIO_NET_F_GUEST_ANNOUNCE = 1 << 21,
		VIRTIO_NET_F_MQ = 1 << 22,
		VIRTIO_NET_F_CTRL_MAC_ADDR = 1 << 23,
		VIRTIO_F_RING_INDIRECT_DESC = 1 << 28,
		VIRTIO_F_RING_EVENT_IDX = 1 << 29,
		VIRTIO_F_VERSION_1 = 1 << 32,
		VIRTIO_F_ACCESS_PLATFORM = 1 << 33,
		VIRTIO_F_RING_PACKED = 1 << 34,
		VIRTIO_F_IN_ORDER = 1 << 35,
		VIRTIO_F_ORDER_PLATFORM = 1 << 36,
		VIRTIO_F_SR_IOV = 1 << 37,
		VIRTIO_F_NOTIFICATION_DATA = 1 << 38,
		VIRTIO_NET_F_GUEST_HDRLEN = 1 << 59,
		VIRTIO_NET_F_RSC_EXT = 1 << 61,
		VIRTIO_NET_F_STANDBY = 1 << 62,
		// INTERNAL DOCUMENTATION TO KNOW WHICH FEATURES HAVE REQUIREMENTS
		//
		// 5.1.3.1 Feature bit requirements
		// Some networking feature bits require other networking feature bits (see 2.2.1):
		// VIRTIO_NET_F_GUEST_TSO4 Requires VIRTIO_NET_F_GUEST_CSUM.
		// VIRTIO_NET_F_GUEST_TSO6 Requires VIRTIO_NET_F_GUEST_CSUM.
		// VIRTIO_NET_F_GUEST_ECN Requires VIRTIO_NET_F_GUEST_TSO4orVIRTIO_NET_F_GUEST_TSO6.
		// VIRTIO_NET_F_GUEST_UFO Requires VIRTIO_NET_F_GUEST_CSUM.
		// VIRTIO_NET_F_HOST_TSO4 Requires VIRTIO_NET_F_CSUM.
		// VIRTIO_NET_F_HOST_TSO6 Requires VIRTIO_NET_F_CSUM.
		// VIRTIO_NET_F_HOST_ECN Requires VIRTIO_NET_F_HOST_TSO4 or VIRTIO_NET_F_HOST_TSO6.
		// VIRTIO_NET_F_HOST_UFO Requires VIRTIO_NET_F_CSUM.
		// VIRTIO_NET_F_CTRL_RX Requires VIRTIO_NET_F_CTRL_VQ.
		// VIRTIO_NET_F_CTRL_VLAN Requires VIRTIO_NET_F_CTRL_VQ.
		// VIRTIO_NET_F_GUEST_ANNOUNCE Requires VIRTIO_NET_F_CTRL_VQ.
		// VIRTIO_NET_F_MQ Requires VIRTIO_NET_F_CTRL_VQ.
		// VIRTIO_NET_F_CTRL_MAC_ADDR Requires VIRTIO_NET_F_CTRL_VQ.
		// VIRTIO_NET_F_RSC_EXT Requires VIRTIO_NET_F_HOST_TSO4 or VIRTIO_NET_F_HOST_TSO6.
	}

	impl From<Features> for u64 {
		fn from(val: Features) -> Self {
			match val {
				Features::VIRTIO_NET_F_CSUM => 1 << 0,
				Features::VIRTIO_NET_F_GUEST_CSUM => 1 << 1,
				Features::VIRTIO_NET_F_CTRL_GUEST_OFFLOADS => 1 << 2,
				Features::VIRTIO_NET_F_MTU => 1 << 3,
				Features::VIRTIO_NET_F_MAC => 1 << 5,
				Features::VIRTIO_NET_F_GUEST_TSO4 => 1 << 7,
				Features::VIRTIO_NET_F_GUEST_TSO6 => 1 << 8,
				Features::VIRTIO_NET_F_GUEST_ECN => 1 << 9,
				Features::VIRTIO_NET_F_GUEST_UFO => 1 << 10,
				Features::VIRTIO_NET_F_HOST_TSO4 => 1 << 11,
				Features::VIRTIO_NET_F_HOST_TSO6 => 1 << 12,
				Features::VIRTIO_NET_F_HOST_ECN => 1 << 13,
				Features::VIRTIO_NET_F_HOST_UFO => 1 << 14,
				Features::VIRTIO_NET_F_MRG_RXBUF => 1 << 15,
				Features::VIRTIO_NET_F_STATUS => 1 << 16,
				Features::VIRTIO_NET_F_CTRL_VQ => 1 << 17,
				Features::VIRTIO_NET_F_CTRL_RX => 1 << 18,
				Features::VIRTIO_NET_F_CTRL_VLAN => 1 << 19,
				Features::VIRTIO_NET_F_GUEST_ANNOUNCE => 1 << 21,
				Features::VIRTIO_NET_F_MQ => 1 << 22,
				Features::VIRTIO_NET_F_CTRL_MAC_ADDR => 1 << 23,
				Features::VIRTIO_F_RING_INDIRECT_DESC => 1 << 28,
				Features::VIRTIO_F_RING_EVENT_IDX => 1 << 29,
				Features::VIRTIO_F_VERSION_1 => 1 << 32,
				Features::VIRTIO_F_ACCESS_PLATFORM => 1 << 33,
				Features::VIRTIO_F_RING_PACKED => 1 << 34,
				Features::VIRTIO_F_IN_ORDER => 1 << 35,
				Features::VIRTIO_F_ORDER_PLATFORM => 1 << 36,
				Features::VIRTIO_F_SR_IOV => 1 << 37,
				Features::VIRTIO_F_NOTIFICATION_DATA => 1 << 38,
				Features::VIRTIO_NET_F_GUEST_HDRLEN => 1 << 59,
				Features::VIRTIO_NET_F_RSC_EXT => 1 << 61,
				Features::VIRTIO_NET_F_STANDBY => 1 << 62,
			}
		}
	}

	impl BitOr for Features {
		type Output = u64;

		fn bitor(self, rhs: Self) -> Self::Output {
			u64::from(self) | u64::from(rhs)
		}
	}

	impl BitOr<Features> for u64 {
		type Output = u64;

		fn bitor(self, rhs: Features) -> Self::Output {
			self | u64::from(rhs)
		}
	}

	impl BitOrAssign<Features> for u64 {
		fn bitor_assign(&mut self, rhs: Features) {
			*self |= u64::from(rhs);
		}
	}

	impl BitAnd for Features {
		type Output = u64;

		fn bitand(self, rhs: Features) -> Self::Output {
			u64::from(self) & u64::from(rhs)
		}
	}

	impl BitAnd<Features> for u64 {
		type Output = u64;

		fn bitand(self, rhs: Features) -> Self::Output {
			self & u64::from(rhs)
		}
	}

	impl BitAndAssign<Features> for u64 {
		fn bitand_assign(&mut self, rhs: Features) {
			*self &= u64::from(rhs);
		}
	}

	impl core::fmt::Display for Features {
		fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
			match *self {
				Features::VIRTIO_NET_F_CSUM => write!(f, "VIRTIO_NET_F_CSUM"),
				Features::VIRTIO_NET_F_GUEST_CSUM => write!(f, "VIRTIO_NET_F_GUEST_CSUM"),
				Features::VIRTIO_NET_F_CTRL_GUEST_OFFLOADS => {
					write!(f, "VIRTIO_NET_F_CTRL_GUEST_OFFLOADS")
				}
				Features::VIRTIO_NET_F_MTU => write!(f, "VIRTIO_NET_F_MTU"),
				Features::VIRTIO_NET_F_MAC => write!(f, "VIRTIO_NET_F_MAC"),
				Features::VIRTIO_NET_F_GUEST_TSO4 => write!(f, "VIRTIO_NET_F_GUEST_TSO4"),
				Features::VIRTIO_NET_F_GUEST_TSO6 => write!(f, "VIRTIO_NET_F_GUEST_TSO6"),
				Features::VIRTIO_NET_F_GUEST_ECN => write!(f, "VIRTIO_NET_F_GUEST_ECN"),
				Features::VIRTIO_NET_F_GUEST_UFO => write!(f, "VIRTIO_NET_FGUEST_UFO"),
				Features::VIRTIO_NET_F_HOST_TSO4 => write!(f, "VIRTIO_NET_F_HOST_TSO4"),
				Features::VIRTIO_NET_F_HOST_TSO6 => write!(f, "VIRTIO_NET_F_HOST_TSO6"),
				Features::VIRTIO_NET_F_HOST_ECN => write!(f, "VIRTIO_NET_F_HOST_ECN"),
				Features::VIRTIO_NET_F_HOST_UFO => write!(f, "VIRTIO_NET_F_HOST_UFO"),
				Features::VIRTIO_NET_F_MRG_RXBUF => write!(f, "VIRTIO_NET_F_MRG_RXBUF"),
				Features::VIRTIO_NET_F_STATUS => write!(f, "VIRTIO_NET_F_STATUS"),
				Features::VIRTIO_NET_F_CTRL_VQ => write!(f, "VIRTIO_NET_F_CTRL_VQ"),
				Features::VIRTIO_NET_F_CTRL_RX => write!(f, "VIRTIO_NET_F_CTRL_RX"),
				Features::VIRTIO_NET_F_CTRL_VLAN => write!(f, "VIRTIO_NET_F_CTRL_VLAN"),
				Features::VIRTIO_NET_F_GUEST_ANNOUNCE => write!(f, "VIRTIO_NET_F_GUEST_ANNOUNCE"),
				Features::VIRTIO_NET_F_MQ => write!(f, "VIRTIO_NET_F_MQ"),
				Features::VIRTIO_NET_F_CTRL_MAC_ADDR => write!(f, "VIRTIO_NET_F_CTRL_MAC_ADDR"),
				Features::VIRTIO_F_RING_INDIRECT_DESC => write!(f, "VIRTIO_F_RING_INDIRECT_DESC"),
				Features::VIRTIO_F_RING_EVENT_IDX => write!(f, "VIRTIO_F_RING_EVENT_IDX"),
				Features::VIRTIO_F_VERSION_1 => write!(f, "VIRTIO_F_VERSION_1"),
				Features::VIRTIO_F_ACCESS_PLATFORM => write!(f, "VIRTIO_F_ACCESS_PLATFORM"),
				Features::VIRTIO_F_RING_PACKED => write!(f, "VIRTIO_F_RING_PACKED"),
				Features::VIRTIO_F_IN_ORDER => write!(f, "VIRTIO_F_IN_ORDER"),
				Features::VIRTIO_F_ORDER_PLATFORM => write!(f, "VIRTIO_F_ORDER_PLATFORM"),
				Features::VIRTIO_F_SR_IOV => write!(f, "VIRTIO_F_SR_IOV"),
				Features::VIRTIO_F_NOTIFICATION_DATA => write!(f, "VIRTIO_F_NOTIFICATION_DATA"),
				Features::VIRTIO_NET_F_GUEST_HDRLEN => write!(f, "VIRTIO_NET_F_GUEST_HDRLEN"),
				Features::VIRTIO_NET_F_RSC_EXT => write!(f, "VIRTIO_NET_F_RSC_EXT"),
				Features::VIRTIO_NET_F_STANDBY => write!(f, "VIRTIO_NET_F_STANDBY"),
			}
		}
	}

	impl Features {
		/// Return a vector of [Features](Features) for a given input of a u64 representation.
		///
		/// INFO: In case the FEATURES enum is changed, this function MUST also be adjusted to the new set!
		//
		// Really UGLY function, but currently the most convenienvt one to reduce the set of features for the driver easily!
		pub fn into_features(feat_set: FeatureSet) -> Option<Vec<Features>> {
			let mut vec_of_feats: Vec<Features> = Vec::new();
			let feats = feat_set.0;

			if feats & (1 << 0) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_CSUM)
			}
			if feats & (1 << 1) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_GUEST_CSUM)
			}
			if feats & (1 << 2) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_CTRL_GUEST_OFFLOADS)
			}
			if feats & (1 << 3) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_MTU)
			}
			if feats & (1 << 5) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_MAC)
			}
			if feats & (1 << 7) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_GUEST_TSO4)
			}
			if feats & (1 << 8) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_GUEST_TSO6)
			}
			if feats & (1 << 9) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_GUEST_ECN)
			}
			if feats & (1 << 10) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_GUEST_UFO)
			}
			if feats & (1 << 11) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_HOST_TSO4)
			}
			if feats & (1 << 12) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_HOST_TSO6)
			}
			if feats & (1 << 13) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_HOST_ECN)
			}
			if feats & (1 << 14) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_HOST_UFO)
			}
			if feats & (1 << 15) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_MRG_RXBUF)
			}
			if feats & (1 << 16) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_STATUS)
			}
			if feats & (1 << 17) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_CTRL_VQ)
			}
			if feats & (1 << 18) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_CTRL_RX)
			}
			if feats & (1 << 19) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_CTRL_VLAN)
			}
			if feats & (1 << 21) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_GUEST_ANNOUNCE)
			}
			if feats & (1 << 22) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_MQ)
			}
			if feats & (1 << 23) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_CTRL_MAC_ADDR)
			}
			if feats & (1 << 28) != 0 {
				vec_of_feats.push(Features::VIRTIO_F_RING_INDIRECT_DESC)
			}
			if feats & (1 << 29) != 0 {
				vec_of_feats.push(Features::VIRTIO_F_RING_EVENT_IDX)
			}
			if feats & (1 << 32) != 0 {
				vec_of_feats.push(Features::VIRTIO_F_VERSION_1)
			}
			if feats & (1 << 33) != 0 {
				vec_of_feats.push(Features::VIRTIO_F_ACCESS_PLATFORM)
			}
			if feats & (1 << 34) != 0 {
				vec_of_feats.push(Features::VIRTIO_F_RING_PACKED)
			}
			if feats & (1 << 35) != 0 {
				vec_of_feats.push(Features::VIRTIO_F_IN_ORDER)
			}
			if feats & (1 << 36) != 0 {
				vec_of_feats.push(Features::VIRTIO_F_ORDER_PLATFORM)
			}
			if feats & (1 << 37) != 0 {
				vec_of_feats.push(Features::VIRTIO_F_SR_IOV)
			}
			if feats & (1 << 38) != 0 {
				vec_of_feats.push(Features::VIRTIO_F_NOTIFICATION_DATA)
			}
			if feats & (1 << 59) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_GUEST_HDRLEN)
			}
			if feats & (1 << 61) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_RSC_EXT)
			}
			if feats & (1 << 62) != 0 {
				vec_of_feats.push(Features::VIRTIO_NET_F_STANDBY)
			}

			if vec_of_feats.is_empty() {
				None
			} else {
				Some(vec_of_feats)
			}
		}
	}

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

	/// FeatureSet is new type whicih holds features for virito network devices indicated by the virtio specification
	/// v1.1. - 5.1.3. and all General Features defined in Virtio specification v1.1. - 6
	/// wrapping a u64.
	///
	/// The main functionality of this type are functions implemented on it.
	#[derive(Debug, Copy, Clone, PartialOrd, PartialEq)]
	pub struct FeatureSet(u64);

	impl BitOr for FeatureSet {
		type Output = FeatureSet;

		fn bitor(self, rhs: Self) -> Self::Output {
			FeatureSet(self.0 | rhs.0)
		}
	}

	impl BitOr<FeatureSet> for u64 {
		type Output = u64;

		fn bitor(self, rhs: FeatureSet) -> Self::Output {
			self | u64::from(rhs)
		}
	}

	impl BitOrAssign<FeatureSet> for u64 {
		fn bitor_assign(&mut self, rhs: FeatureSet) {
			*self |= u64::from(rhs);
		}
	}

	impl BitOrAssign<Features> for FeatureSet {
		fn bitor_assign(&mut self, rhs: Features) {
			self.0 = self.0 | u64::from(rhs);
		}
	}

	impl BitAnd for FeatureSet {
		type Output = FeatureSet;

		fn bitand(self, rhs: FeatureSet) -> Self::Output {
			FeatureSet(self.0 & rhs.0)
		}
	}

	impl BitAnd<FeatureSet> for u64 {
		type Output = u64;

		fn bitand(self, rhs: FeatureSet) -> Self::Output {
			self & u64::from(rhs)
		}
	}

	impl BitAndAssign<FeatureSet> for u64 {
		fn bitand_assign(&mut self, rhs: FeatureSet) {
			*self &= u64::from(rhs);
		}
	}

	impl From<FeatureSet> for u64 {
		fn from(feature_set: FeatureSet) -> Self {
			feature_set.0
		}
	}

	impl FeatureSet {
		/// Checks if a given set of features is compatible and adheres to the
		/// specfification v1.1. - 5.1.3.1
		/// Upon an error returns the incompatible set of features by the
		/// [FeatReqNotMet](self::error::VirtioNetError) errror value, which
		/// wraps the u64 indicating the feature set.
		///
		/// INFO: Iterates twice over the vector of features.
		pub fn check_features(feats: &Vec<Features>) -> Result<(), VirtioNetError> {
			let mut feat_bits = 0u64;

			for feat in feats.iter() {
				feat_bits |= *feat;
			}

			for feat in feats {
				match feat {
					Features::VIRTIO_NET_F_CSUM => continue,
					Features::VIRTIO_NET_F_GUEST_CSUM => continue,
					Features::VIRTIO_NET_F_CTRL_GUEST_OFFLOADS => continue,
					Features::VIRTIO_NET_F_MTU => continue,
					Features::VIRTIO_NET_F_MAC => continue,
					Features::VIRTIO_NET_F_GUEST_TSO4 => {
						if feat_bits & Features::VIRTIO_NET_F_GUEST_CSUM != 0 {
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_GUEST_TSO6 => {
						if feat_bits & Features::VIRTIO_NET_F_GUEST_CSUM != 0 {
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_GUEST_ECN => {
						if feat_bits
							& (Features::VIRTIO_NET_F_GUEST_TSO4
								| Features::VIRTIO_NET_F_GUEST_TSO6)
							!= 0
						{
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_GUEST_UFO => {
						if feat_bits & Features::VIRTIO_NET_F_GUEST_CSUM != 0 {
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_HOST_TSO4 => {
						if feat_bits & Features::VIRTIO_NET_F_CSUM != 0 {
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_HOST_TSO6 => {
						if feat_bits & Features::VIRTIO_NET_F_CSUM != 0 {
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_HOST_ECN => {
						if feat_bits
							& (Features::VIRTIO_NET_F_HOST_TSO4 | Features::VIRTIO_NET_F_HOST_TSO6)
							!= 0
						{
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_HOST_UFO => {
						if feat_bits & Features::VIRTIO_NET_F_CSUM != 0 {
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_MRG_RXBUF => continue,
					Features::VIRTIO_NET_F_STATUS => continue,
					Features::VIRTIO_NET_F_CTRL_VQ => continue,
					Features::VIRTIO_NET_F_CTRL_RX => {
						if feat_bits & Features::VIRTIO_NET_F_CTRL_VQ != 0 {
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_CTRL_VLAN => {
						if feat_bits & Features::VIRTIO_NET_F_CTRL_VQ != 0 {
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_GUEST_ANNOUNCE => {
						if feat_bits & Features::VIRTIO_NET_F_CTRL_VQ != 0 {
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_MQ => {
						if feat_bits & Features::VIRTIO_NET_F_CTRL_VQ != 0 {
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_CTRL_MAC_ADDR => {
						if feat_bits & Features::VIRTIO_NET_F_CTRL_VQ != 0 {
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_GUEST_HDRLEN => continue,
					Features::VIRTIO_NET_F_RSC_EXT => {
						if feat_bits
							& (Features::VIRTIO_NET_F_HOST_TSO4 | Features::VIRTIO_NET_F_HOST_TSO6)
							!= 0
						{
							continue;
						} else {
							return Err(VirtioNetError::FeatReqNotMet(FeatureSet(feat_bits)));
						}
					}
					Features::VIRTIO_NET_F_STANDBY => continue,
					Features::VIRTIO_F_RING_INDIRECT_DESC => continue,
					Features::VIRTIO_F_RING_EVENT_IDX => continue,
					Features::VIRTIO_F_VERSION_1 => continue,
					Features::VIRTIO_F_ACCESS_PLATFORM => continue,
					Features::VIRTIO_F_RING_PACKED => continue,
					Features::VIRTIO_F_IN_ORDER => continue,
					Features::VIRTIO_F_ORDER_PLATFORM => continue,
					Features::VIRTIO_F_SR_IOV => continue,
					Features::VIRTIO_F_NOTIFICATION_DATA => continue,
				}
			}

			Ok(())
		}

		/// Checks if a given feature is set.
		pub fn is_feature(self, feat: Features) -> bool {
			self.0 & feat != 0
		}

		/// Sets features contained in feats to true.
		///
		/// WARN: Features should be checked before using this function via the
		/// `FeatureSet::check_features(feats: Vec<Features>) -> Result<(), VirtioNetError>` function.
		pub fn set_features(&mut self, feats: &Vec<Features>) {
			for feat in feats {
				self.0 |= *feat;
			}
		}

		/// Returns a new instance of (FeatureSet)[FeatureSet] with all features
		/// initialized to false.
		pub fn new(val: u64) -> Self {
			FeatureSet(val)
		}
	}
}

/// Error module of virtios network driver. Containing the (VirtioNetError)[VirtioNetError]
/// enum.
pub mod error {
	use super::constants::FeatureSet;
	/// Network drivers error enum.
	#[derive(Debug, Copy, Clone)]
	pub enum VirtioNetError {
		General,
		NoDevCfg(u16),
		NoComCfg(u16),
		NoIsrCfg(u16),
		NoNotifCfg(u16),
		FailFeatureNeg(u16),
		/// Set of features does not adhere to the requirements of features
		/// indicated by the specification
		FeatReqNotMet(FeatureSet),
		/// The first u64 contains the feature bits wanted by the driver.
		/// but which are incompatible with the device feature set, second u64.
		IncompFeatsSet(FeatureSet, FeatureSet),
		/// Indicates that an operation for finished Transfers, was performed on
		/// an ongoing transfer
		ProcessOngoing,
	}
}
