//! This module contains Virtio's packed virtqueue.
//! See Virito specification v1.1. - 2.7
#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use core::mem::MaybeUninit;
use core::sync::atomic::{fence, Ordering};
use core::{iter, ops, ptr};

use align_address::Align;
use virtio::pci::NotificationData;
use virtio::pvirtq::{EventSuppressDesc, EventSuppressFlags};
use virtio::{pvirtq, virtq, RingEventFlags};

#[cfg(not(feature = "pci"))]
use super::super::transport::mmio::{ComCfg, NotifCfg, NotifCtrl};
#[cfg(feature = "pci")]
use super::super::transport::pci::{ComCfg, NotifCfg, NotifCtrl};
use super::error::VirtqError;
use super::{
	BuffSpec, Buffer, BufferToken, BufferType, Bytes, MemDescr, MemDescrId, MemPool, TransferToken,
	Virtq, VirtqPrivate, VqIndex, VqSize,
};
use crate::arch::mm::paging::{BasePageSize, PageSize};
use crate::arch::mm::{paging, VirtAddr};

#[derive(Default, PartialEq, Eq, Clone, Copy, Debug)]
struct RingIdx {
	off: u16,
	wrap: u8,
}

trait RingIndexRange {
	fn wrapping_contains(&self, item: &RingIdx) -> bool;
}

impl RingIndexRange for ops::Range<RingIdx> {
	fn wrapping_contains(&self, item: &RingIdx) -> bool {
		let ops::Range { start, end } = self;

		if start.wrap == end.wrap {
			item.wrap == start.wrap && start.off <= item.off && item.off < end.off
		} else if item.wrap == start.wrap {
			start.off <= item.off
		} else {
			debug_assert!(item.wrap == end.wrap);
			item.off < end.off
		}
	}
}

/// A newtype of bool used for convenience in context with
/// packed queues wrap counter.
///
/// For more details see Virtio specification v1.1. - 2.7.1
#[derive(Copy, Clone, Debug)]
struct WrapCount(bool);

impl WrapCount {
	/// Masks all other bits, besides the wrap count specific ones.
	fn flag_mask() -> virtq::DescF {
		virtq::DescF::AVAIL | virtq::DescF::USED
	}

	/// Returns a new WrapCount struct initialized to true or 1.
	///
	/// See virtio specification v1.1. - 2.7.1
	fn new() -> Self {
		WrapCount(true)
	}

	/// Toogles a given wrap count to respectiver other value.
	///
	/// If WrapCount(true) returns WrapCount(false),
	/// if WrapCount(false) returns WrapCount(true).
	fn wrap(&mut self) {
		self.0 = !self.0
	}

	/// Creates avail and used flags inside u16 in accordance to the
	/// virito specification v1.1. - 2.7.1
	///
	/// I.e.: Set avail flag to match the WrapCount and the used flag
	/// to NOT match the WrapCount.
	fn as_flags_avail(&self) -> virtq::DescF {
		if self.0 {
			virtq::DescF::AVAIL
		} else {
			virtq::DescF::USED
		}
	}

	/// Creates avail and used flags inside u16 in accordance to the
	/// virito specification v1.1. - 2.7.1
	///
	/// I.e.: Set avail flag to match the WrapCount and the used flag
	/// to also match the WrapCount.
	fn as_flags_used(&self) -> virtq::DescF {
		if self.0 {
			virtq::DescF::AVAIL | virtq::DescF::USED
		} else {
			virtq::DescF::empty()
		}
	}
}

/// Structure which allows to control raw ring and operate easily on it
struct DescriptorRing {
	ring: &'static mut [pvirtq::Desc],
	tkn_ref_ring: Box<[Option<Box<TransferToken>>]>,

	// Controlling variables for the ring
	//
	/// where to insert available descriptors next
	write_index: u16,
	/// How much descriptors can be inserted
	capacity: u16,
	/// Where to expect the next used descriptor by the device
	poll_index: u16,
	/// See Virtio specification v1.1. - 2.7.1
	drv_wc: WrapCount,
	dev_wc: WrapCount,
	/// Memory pool controls the amount of "free floating" descriptors
	/// See [MemPool] docs for detail.
	mem_pool: MemPool,
}

impl DescriptorRing {
	fn new(size: u16) -> Self {
		// Allocate heap memory via a vec, leak and cast
		let _mem_len = (usize::from(size) * core::mem::size_of::<pvirtq::Desc>())
			.align_up(BasePageSize::SIZE as usize);
		let ptr = ptr::with_exposed_provenance_mut(crate::mm::allocate(_mem_len, true).0 as usize);

		let ring: &'static mut [pvirtq::Desc] =
			unsafe { core::slice::from_raw_parts_mut(ptr, size.into()) };

		// `Box` is not Clone, so neither is `None::<Box<_>>`. Hence, we need to produce `None`s with a closure.
		let tkn_ref_ring = core::iter::repeat_with(|| None)
			.take(size.into())
			.collect::<Vec<_>>()
			.into_boxed_slice();

		DescriptorRing {
			ring,
			tkn_ref_ring,
			write_index: 0,
			capacity: size,
			poll_index: 0,
			drv_wc: WrapCount::new(),
			dev_wc: WrapCount::new(),
			mem_pool: MemPool::new(size),
		}
	}

	/// Polls poll index and sets the state of any finished TransferTokens.
	/// If [TransferToken::await_queue] is available, the [BufferToken] will be moved to the queue.
	fn poll(&mut self) {
		let mut ctrl = self.get_read_ctrler();

		if let Some(mut tkn) = ctrl.poll_next() {
			if let Some(queue) = tkn.await_queue.take() {
				// Place the TransferToken in a Transfer, which will hold ownership of the token
				queue.try_send(Box::new(tkn.buff_tkn)).unwrap();
			}
		}
	}

	fn push_batch(&mut self, tkn_lst: Vec<TransferToken>) -> Result<RingIdx, VirtqError> {
		// Catch empty push, in order to allow zero initialized first_ctrl_settings struct
		// which will be overwritten in the first iteration of the for-loop
		assert!(!tkn_lst.is_empty());

		let mut first_ctrl_settings: (u16, MemDescrId, WrapCount) =
			(0, MemDescrId(0), WrapCount::new());
		let mut first_buffer = None;

		for (i, tkn) in tkn_lst.into_iter().enumerate() {
			let mut ctrl = self.push_without_making_available(&tkn)?;
			if i == 0 {
				first_ctrl_settings = (ctrl.start, ctrl.buff_id, ctrl.wrap_at_init);
				first_buffer = Some(Box::new(tkn));
			} else {
				// Update flags of the first descriptor and set new write_index
				ctrl.make_avail(Box::new(tkn));
			}
		}
		// Manually make the first buffer available lastly
		//
		// Providing the first buffer in the list manually
		self.make_avail_with_state(
			first_buffer.unwrap(),
			first_ctrl_settings.0,
			first_ctrl_settings.1,
			first_ctrl_settings.2,
		);
		Ok(RingIdx {
			off: self.write_index,
			wrap: self.drv_wc.0.into(),
		})
	}

	fn push(&mut self, tkn: TransferToken) -> Result<RingIdx, VirtqError> {
		let mut ctrl = self.push_without_making_available(&tkn)?;
		// Update flags of the first descriptor and set new write_index
		ctrl.make_avail(Box::new(tkn));

		Ok(RingIdx {
			off: self.write_index,
			wrap: self.drv_wc.0.into(),
		})
	}

	fn push_without_making_available(
		&mut self,
		tkn: &TransferToken,
	) -> Result<WriteCtrl<'_>, VirtqError> {
		// Check length and if its fits. This should always be true due to the restriction of
		// the memory pool, but to be sure.
		assert!(tkn.num_consuming_descr() <= self.capacity);

		// create an counter that wrappes to the first element
		// after reaching a the end of the ring
		let mut ctrl = self.get_write_ctrler()?;

		// Importance here is:
		// * distinguish between Indirect and direct buffers
		// * make them available in the right order (the first descriptor last) (VIRTIO Spec. v1.2 section 2.8.6)

		// The buffer uses indirect descriptors if the ctrl_desc field is Some.
		if let Some(ctrl_desc) = tkn.ctrl_desc.as_ref() {
			// One indirect descriptor with only flag indirect set
			ctrl.write_desc(ctrl_desc, virtq::DescF::INDIRECT);
		} else {
			let send_desc_iter = tkn
				.buff_tkn
				.send_buff
				.as_ref()
				.map(|send_buff| send_buff.as_slice().iter())
				.into_iter()
				.flatten()
				.zip(iter::repeat(virtq::DescF::empty()));
			let recv_desc_iter = tkn
				.buff_tkn
				.recv_buff
				.as_ref()
				.map(|recv_buff| recv_buff.as_slice().iter())
				.into_iter()
				.flatten()
				.zip(iter::repeat(virtq::DescF::WRITE));
			let mut all_desc_iter = send_desc_iter.chain(recv_desc_iter);
			// We take all but the last pair to
			for (desc, incomplete_flag) in all_desc_iter
				.by_ref()
				.take(usize::from(tkn.buff_tkn.num_descr()) - 1)
			{
				ctrl.write_desc(desc, incomplete_flag | virtq::DescF::NEXT);
			}
			// The iterator should have left the last element, as we took one less than what is available.
			let (last_desc, last_flag) = all_desc_iter.next().unwrap();
			ctrl.write_desc(last_desc, last_flag);
		}
		Ok(ctrl)
	}

	/// # Unsafe
	/// Returns the memory address of the first element of the descriptor ring
	fn raw_addr(&self) -> usize {
		self.ring.as_ptr() as usize
	}

	/// Returns an initialized write controller in order
	/// to write the queue correctly.
	fn get_write_ctrler(&mut self) -> Result<WriteCtrl<'_>, VirtqError> {
		let desc_id = self
			.mem_pool
			.pool
			.borrow_mut()
			.pop()
			.ok_or(VirtqError::NoDescrAvail)?;
		Ok(WriteCtrl {
			start: self.write_index,
			position: self.write_index,
			modulo: u16::try_from(self.ring.len()).unwrap(),
			wrap_at_init: self.drv_wc,
			buff_id: desc_id,

			desc_ring: self,
		})
	}

	/// Returns an initialized read controller in order
	/// to read the queue correctly.
	fn get_read_ctrler(&mut self) -> ReadCtrl<'_> {
		ReadCtrl {
			position: self.poll_index,
			modulo: u16::try_from(self.ring.len()).unwrap(),

			desc_ring: self,
		}
	}

	fn make_avail_with_state(
		&mut self,
		raw_tkn: Box<TransferToken>,
		start: u16,
		buff_id: MemDescrId,
		wrap_at_init: WrapCount,
	) {
		// provide reference, in order to let TransferToken know upon finish.
		self.tkn_ref_ring[usize::from(buff_id.0)] = Some(raw_tkn);
		// The driver performs a suitable memory barrier to ensure the device sees the updated descriptor table and available ring before the next step.
		// See Virtio specfification v1.1. - 2.7.21
		fence(Ordering::SeqCst);
		self.ring[usize::from(start)].flags |= wrap_at_init.as_flags_avail();
	}
}

struct ReadCtrl<'a> {
	/// Poll index of the ring at init of ReadCtrl
	position: u16,
	modulo: u16,

	desc_ring: &'a mut DescriptorRing,
}

impl<'a> ReadCtrl<'a> {
	/// Polls the ring for a new finished buffer. If buffer is marked as finished, takes care of
	/// updating the queue and returns the respective TransferToken.
	fn poll_next(&mut self) -> Option<Box<TransferToken>> {
		// Check if descriptor has been marked used.
		if self.desc_ring.ring[usize::from(self.position)].flags & WrapCount::flag_mask()
			== self.desc_ring.dev_wc.as_flags_used()
		{
			let buff_id = self.desc_ring.ring[usize::from(self.position)].id.to_ne();
			let mut tkn = self.desc_ring.tkn_ref_ring[usize::from(buff_id)]
				.take()
				.expect(
					"The buff_id is incorrect or the reference to the TransferToken was misplaced.",
				);

			let (send_buff, recv_buff) = {
				let BufferToken {
					send_buff,
					recv_buff,
					..
				} = &mut tkn.buff_tkn;
				(recv_buff.as_mut(), send_buff.as_mut())
			};

			// Retrieve if any has been written to the queue. If this is the case, we calculate the overall length
			// This is necessary in order to provide the drivers with the correct access, to usable data.
			//
			// According to the standard the device signals solely via the first written descriptor if anything has been written to
			// the write descriptors of a buffer.
			// See Virtio specification v1.1. - 2.7.4
			//                                - 2.7.5
			//                                - 2.7.6
			// let mut write_len = if self.desc_ring.ring[self.position].flags & DescrFlags::VIRTQ_DESC_F_WRITE == DescrFlags::VIRTQ_DESC_F_WRITE {
			//      self.desc_ring.ring[self.position].len
			//  } else {
			//      0
			//  };
			//
			// INFO:
			// Due to the behaviour of the currently used devices and the virtio code from the linux kernel, we assume, that device do NOT set this
			// flag correctly upon writes. Hence we omit it, in order to receive data.
			let write_len = self.desc_ring.ring[usize::from(self.position)].len;

			if tkn.ctrl_desc.is_some() {
				if let Some(recv_buff) = recv_buff {
					self.update_indirect(recv_buff, write_len.into());
				}
			} else {
				if let Some(send_buff) = send_buff {
					self.update_send(send_buff);
				}
				if let Some(recv_buff) = recv_buff {
					self.update_recv((recv_buff, write_len.into()));
				}
			}
			self.desc_ring.mem_pool.ret_id(MemDescrId(buff_id));
			Some(tkn)
		} else {
			None
		}
	}

	/// Updates the accessible len of the memory areas accessible by the drivers to be consistent with
	/// the amount of data written by the device.
	///
	/// Indirect descriptor tables are read-only for devices. Hence all information comes from the
	/// used descriptor in the actual ring.
	fn update_indirect(&mut self, recv_buff: &mut Buffer, write_len: u32) {
		let mut write_len = usize::try_from(write_len).unwrap();

		recv_buff.restr_len(write_len);

		for desc in recv_buff.as_mut_slice() {
			if write_len >= desc.len {
				// Complete length has been written but reduce len_written for next one
				write_len -= desc.len;
			} else {
				desc.len = write_len;
				write_len -= desc.len;
				assert_eq!(write_len, 0);
			}
		}

		// Increase poll_index and reset ring position beforehand in order to have a consistent and clean
		// data structure.
		self.reset_ring_pos();
		self.incrmt();
	}

	/// Resets the current position in the ring in order to have a consistent data structure.
	///
	/// This does currently NOT include, resetting address, len and buff_id.
	fn reset_ring_pos(&mut self) {
		// self.desc_ring.ring[self.position].address = 0;
		// self.desc_ring.ring[self.position].len = 0;
		// self.desc_ring.ring[self.position].buff_id = 0;
		self.desc_ring.ring[usize::from(self.position)].flags =
			self.desc_ring.dev_wc.as_flags_used();
	}

	/// Updates the accessible len of the memory areas accessible by the drivers to be consistent with
	/// the amount of data written by the device.
	/// Updates the descriptor flags inside the actual ring if necessary and
	/// increments the poll_index by one.
	///
	/// The given buffer must NEVER be an indirect buffer.
	fn update_recv(&mut self, recv_buff_spec: (&mut Buffer, u32)) {
		let (recv_buff, write_len) = recv_buff_spec;
		let mut write_len = usize::try_from(write_len).unwrap();

		recv_buff.restr_len(write_len);

		for desc in recv_buff.as_mut_slice() {
			if write_len >= desc.len {
				// Complete length has been written but reduce len_written for next one
				write_len -= desc.len;
			} else {
				desc.len = write_len;
				write_len -= desc.len;
				assert_eq!(write_len, 0);
			}

			// Increase poll_index and reset ring position beforehand in order to have a consistent and clean
			// data structure.
			self.reset_ring_pos();
			self.incrmt();
		}
	}

	/// Updates the descriptor flags inside the actual ring if necessary and
	/// increments the poll_index by one.
	fn update_send(&mut self, send_buff: &Buffer) {
		for _desc in send_buff.as_slice() {
			// Increase poll_index and reset ring position beforehand in order to have a consistent and clean
			// data structure.
			self.reset_ring_pos();
			self.incrmt();
		}
	}

	fn incrmt(&mut self) {
		if self.desc_ring.poll_index + 1 == self.modulo {
			self.desc_ring.dev_wc.wrap()
		}

		// Increment capcity as we have one more free now!
		assert!(self.desc_ring.capacity <= u16::try_from(self.desc_ring.ring.len()).unwrap());
		self.desc_ring.capacity += 1;

		self.desc_ring.poll_index = (self.desc_ring.poll_index + 1) % self.modulo;
		self.position = self.desc_ring.poll_index;
	}
}

/// Convenient struct that allows to conveniently write descriptors into the queue.
/// The struct takes care of updating the state of the queue correctly and to write
/// the correct flags.
struct WriteCtrl<'a> {
	/// Where did the write of the buffer start in the descriptor ring
	/// This is important, as we must make this descriptor available
	/// lastly.
	start: u16,
	/// Where to write next. This should always be equal to the Rings
	/// write_next field.
	position: u16,
	modulo: u16,
	/// What was the WrapCount at the first write position
	/// Important in order to set the right avail and used flags
	wrap_at_init: WrapCount,
	/// Buff ID of this write
	buff_id: MemDescrId,

	desc_ring: &'a mut DescriptorRing,
}

impl<'a> WriteCtrl<'a> {
	/// **This function MUST only be used within the WriteCtrl.write_desc() function!**
	///
	/// Incrementing index by one. The index wrappes around to zero when
	/// reaching (modulo -1).
	///
	/// Also takes care of wrapping the WrapCount of the associated
	/// DescriptorRing.
	fn incrmt(&mut self) {
		// Firstly check if we are at all allowed to write a descriptor
		assert!(self.desc_ring.capacity != 0);
		self.desc_ring.capacity -= 1;
		// check if increment wrapped around end of ring
		// then also wrap the wrap counter.
		if self.position + 1 == self.modulo {
			self.desc_ring.drv_wc.wrap();
		}
		// Also update the write_index
		self.desc_ring.write_index = (self.desc_ring.write_index + 1) % self.modulo;

		self.position = (self.position + 1) % self.modulo;
	}

	/// Writes a descriptor of a buffer into the queue. At the correct position, and
	/// with the given flags.
	/// * Flags for avail and used will be set by the queue itself.
	///   * -> Only set different flags here.
	fn write_desc(&mut self, mem_desc: &MemDescr, flags: virtq::DescF) {
		// This also sets the buff_id for the WriteCtrl struct to the ID of the first
		// descriptor.
		let desc_ref = &mut self.desc_ring.ring[usize::from(self.position)];
		desc_ref.addr = paging::virt_to_phys(VirtAddr::from(mem_desc.ptr as u64))
			.as_u64()
			.into();
		desc_ref.len = (mem_desc.len as u32).into();
		desc_ref.id = self.buff_id.0.into();
		if self.start == self.position {
			// Remove possibly set avail and used flags
			desc_ref.flags = flags - virtq::DescF::AVAIL - virtq::DescF::USED;
		} else {
			// Remove possibly set avail and used flags and then set avail and used
			// according to the current WrapCount.
			desc_ref.flags = (flags - virtq::DescF::AVAIL - virtq::DescF::USED)
				| self.desc_ring.drv_wc.as_flags_avail();
		}
		self.incrmt()
	}

	fn make_avail(&mut self, raw_tkn: Box<TransferToken>) {
		// We fail if one wants to make a buffer available without inserting one element!
		assert!(self.start != self.position);
		self.desc_ring
			.make_avail_with_state(raw_tkn, self.start, self.buff_id, self.wrap_at_init);
	}
}

/// A newtype in order to implement the correct functionality upon
/// the `EventSuppr` structure for driver notifications settings.
/// The Driver Event Suppression structure is read-only by the device
/// and controls the used buffer notifications sent by the device to the driver.
struct DrvNotif {
	/// Indicates if VIRTIO_F_RING_EVENT_IDX has been negotiated
	f_notif_idx: bool,
	/// Actual structure to read from, if device wants notifs
	raw: &'static mut pvirtq::EventSuppress,
}

/// A newtype in order to implement the correct functionality upon
/// the `EventSuppr` structure for device notifications settings.
/// The Device Event Suppression structure is read-only by the driver
/// and controls the available buffer notifica- tions sent by the driver to the device.
struct DevNotif {
	/// Indicates if VIRTIO_F_RING_EVENT_IDX has been negotiated
	f_notif_idx: bool,
	/// Actual structure to read from, if device wants notifs
	raw: &'static mut pvirtq::EventSuppress,
}

impl DrvNotif {
	/// Enables notifications by unsetting the LSB.
	/// See Virito specification v1.1. - 2.7.10
	fn enable_notif(&mut self) {
		self.raw.flags = EventSuppressFlags::new().with_desc_event_flags(RingEventFlags::Enable);
	}

	/// Disables notifications by setting the LSB.
	/// See Virtio specification v1.1. - 2.7.10
	fn disable_notif(&mut self) {
		self.raw.flags = EventSuppressFlags::new().with_desc_event_flags(RingEventFlags::Disable);
	}

	/// Enables a notification by the device for a specific descriptor.
	fn enable_specific(&mut self, idx: RingIdx) {
		// Check if VIRTIO_F_RING_EVENT_IDX has been negotiated
		if self.f_notif_idx {
			self.raw.flags = EventSuppressFlags::new().with_desc_event_flags(RingEventFlags::Desc);
			self.raw.desc = EventSuppressDesc::new()
				.with_desc_event_off(idx.off)
				.with_desc_event_wrap(idx.wrap);
		}
	}
}

impl DevNotif {
	/// Enables the notificication capability for a specific buffer.
	pub fn enable_notif_specific(&mut self) {
		self.f_notif_idx = true;
	}

	/// Reads notification bit (i.e. LSB) and returns value.
	/// If notifications are enabled returns true, else false.
	fn is_notif(&self) -> bool {
		self.raw.flags.desc_event_flags() == RingEventFlags::Enable
	}

	fn notif_specific(&self) -> Option<RingIdx> {
		if !self.f_notif_idx {
			return None;
		}

		if self.raw.flags.desc_event_flags() != RingEventFlags::Desc {
			return None;
		}

		let off = self.raw.desc.desc_event_off();
		let wrap = self.raw.desc.desc_event_wrap();

		Some(RingIdx { off, wrap })
	}
}

/// Packed virtqueue which provides the functionilaty as described in the
/// virtio specification v1.1. - 2.7
pub struct PackedVq {
	/// Ring which allows easy access to the raw ring structure of the
	/// specfification
	descr_ring: RefCell<DescriptorRing>,
	/// Allows to tell the device if notifications are wanted
	drv_event: RefCell<DrvNotif>,
	/// Allows to check, if the device wants a notification
	dev_event: DevNotif,
	/// Actually notify device about avail buffers
	notif_ctrl: NotifCtrl,
	/// The size of the queue, equals the number of descriptors which can
	/// be used
	size: VqSize,
	/// The virtqueues index. This identifies the virtqueue to the
	/// device and is unique on a per device basis.
	index: VqIndex,
	last_next: Cell<RingIdx>,
}

// Public interface of PackedVq
// This interface is also public in order to allow people to use the PackedVq directly!
// This is currently unlikely, as the Tokens hold a Rc<Virtq> for refering to their origin
// queue. This could be eased
impl Virtq for PackedVq {
	fn enable_notifs(&self) {
		self.drv_event.borrow_mut().enable_notif();
	}

	fn disable_notifs(&self) {
		self.drv_event.borrow_mut().disable_notif();
	}

	fn poll(&self) {
		self.descr_ring.borrow_mut().poll();
	}

	fn dispatch_batch(&self, tkns: Vec<TransferToken>, notif: bool) -> Result<(), VirtqError> {
		// Zero transfers are not allowed
		assert!(!tkns.is_empty());

		let next_idx = self.descr_ring.borrow_mut().push_batch(tkns)?;

		if notif {
			self.drv_event.borrow_mut().enable_specific(next_idx);
		}

		let range = self.last_next.get()..next_idx;
		let notif_specific = self
			.dev_event
			.notif_specific()
			.map_or(false, |idx| range.wrapping_contains(&idx));

		if self.dev_event.is_notif() || notif_specific {
			let notification_data = NotificationData::new()
				.with_vqn(self.index.0)
				.with_next_off(next_idx.off)
				.with_next_wrap(next_idx.wrap);
			self.notif_ctrl.notify_dev(notification_data);
			self.last_next.set(next_idx);
		}
		Ok(())
	}

	fn dispatch_batch_await(
		&self,
		mut tkns: Vec<TransferToken>,
		await_queue: super::BufferTokenSender,
		notif: bool,
	) -> Result<(), VirtqError> {
		// Zero transfers are not allowed
		assert!(!tkns.is_empty());

		// We have to iterate here too, in order to ensure, tokens are placed into the await_queue
		for tkn in tkns.iter_mut() {
			tkn.await_queue = Some(await_queue.clone());
		}

		let next_idx = self.descr_ring.borrow_mut().push_batch(tkns)?;

		if notif {
			self.drv_event.borrow_mut().enable_specific(next_idx);
		}

		let range = self.last_next.get()..next_idx;
		let notif_specific = self
			.dev_event
			.notif_specific()
			.map_or(false, |idx| range.wrapping_contains(&idx));

		if self.dev_event.is_notif() | notif_specific {
			let notification_data = NotificationData::new()
				.with_vqn(self.index.0)
				.with_next_off(next_idx.off)
				.with_next_wrap(next_idx.wrap);
			self.notif_ctrl.notify_dev(notification_data);
			self.last_next.set(next_idx);
		}
		Ok(())
	}

	fn dispatch(&self, tkn: TransferToken, notif: bool) -> Result<(), VirtqError> {
		let next_idx = self.descr_ring.borrow_mut().push(tkn)?;

		if notif {
			self.drv_event.borrow_mut().enable_specific(next_idx);
		}

		let notif_specific = self.dev_event.notif_specific() == Some(self.last_next.get());

		if self.dev_event.is_notif() || notif_specific {
			let notification_data = NotificationData::new()
				.with_vqn(self.index.0)
				.with_next_off(next_idx.off)
				.with_next_wrap(next_idx.wrap);
			self.notif_ctrl.notify_dev(notification_data);
			self.last_next.set(next_idx);
		}
		Ok(())
	}

	fn index(&self) -> VqIndex {
		self.index
	}

	fn new(
		com_cfg: &mut ComCfg,
		notif_cfg: &NotifCfg,
		size: VqSize,
		index: VqIndex,
		features: virtio::F,
	) -> Result<Self, VirtqError> {
		// Currently we do not have support for in order use.
		// This steems from the fact, that the packedVq ReadCtrl currently is not
		// able to derive other finished transfer from a used-buffer notification.
		// In order to allow this, the queue MUST track the sequence in which
		// TransferTokens are inserted into the queue. Furthermore the Queue should
		// carry a feature u64 in order to check which features are used currently
		// and adjust its ReadCtrl accordingly.
		if features.contains(virtio::F::IN_ORDER) {
			info!("PackedVq has no support for VIRTIO_F_IN_ORDER. Aborting...");
			return Err(VirtqError::FeatureNotSupported(virtio::F::IN_ORDER));
		}

		// Get a handler to the queues configuration area.
		let mut vq_handler = match com_cfg.select_vq(index.into()) {
			Some(handler) => handler,
			None => return Err(VirtqError::QueueNotExisting(index.into())),
		};

		// Must catch zero size as it is not allowed for packed queues.
		// Must catch size larger 32768 (2^15) as it is not allowed for packed queues.
		//
		// See Virtio specification v1.1. - 4.1.4.3.2
		let vq_size = if (size.0 == 0) | (size.0 > 32768) {
			return Err(VirtqError::QueueSizeNotAllowed(size.0));
		} else {
			vq_handler.set_vq_size(size.0)
		};

		let descr_ring = RefCell::new(DescriptorRing::new(vq_size));
		// Allocate heap memory via a vec, leak and cast
		let _mem_len =
			core::mem::size_of::<pvirtq::EventSuppress>().align_up(BasePageSize::SIZE as usize);

		let drv_event_ptr =
			ptr::with_exposed_provenance_mut(crate::mm::allocate(_mem_len, true).0 as usize);
		let dev_event_ptr =
			ptr::with_exposed_provenance_mut(crate::mm::allocate(_mem_len, true).0 as usize);

		// Provide memory areas of the queues data structures to the device
		vq_handler.set_ring_addr(paging::virt_to_phys(VirtAddr::from(
			descr_ring.borrow().raw_addr() as u64,
		)));
		// As usize is safe here, as the *mut EventSuppr raw pointer is a thin pointer of size usize
		vq_handler.set_drv_ctrl_addr(paging::virt_to_phys(VirtAddr::from(drv_event_ptr as u64)));
		vq_handler.set_dev_ctrl_addr(paging::virt_to_phys(VirtAddr::from(dev_event_ptr as u64)));

		let drv_event: &'static mut pvirtq::EventSuppress = unsafe { &mut *(drv_event_ptr) };

		let dev_event: &'static mut pvirtq::EventSuppress = unsafe { &mut *(dev_event_ptr) };

		let drv_event = RefCell::new(DrvNotif {
			f_notif_idx: false,
			raw: drv_event,
		});

		let dev_event = DevNotif {
			f_notif_idx: false,
			raw: dev_event,
		};

		let mut notif_ctrl = NotifCtrl::new(notif_cfg.notification_location(&mut vq_handler));

		if features.contains(virtio::F::NOTIFICATION_DATA) {
			notif_ctrl.enable_notif_data();
		}

		if features.contains(virtio::F::EVENT_IDX) {
			drv_event.borrow_mut().f_notif_idx = true;
		}

		vq_handler.enable_queue();

		info!("Created PackedVq: idx={}, size={}", index.0, vq_size);

		Ok(PackedVq {
			descr_ring,
			drv_event,
			dev_event,
			notif_ctrl,
			size: VqSize::from(vq_size),
			index,
			last_next: Default::default(),
		})
	}

	fn prep_transfer_from_raw(
		self: Rc<Self>,
		send: &[&[u8]],
		recv: &[&mut [MaybeUninit<u8>]],
		buffer_type: BufferType,
	) -> Result<TransferToken, VirtqError> {
		self.prep_transfer_from_raw_static(send, recv, buffer_type)
	}

	fn prep_buffer(
		self: Rc<Self>,
		send: Option<BuffSpec<'_>>,
		recv: Option<BuffSpec<'_>>,
	) -> Result<BufferToken, VirtqError> {
		self.prep_buffer_static(send, recv)
	}

	fn size(&self) -> VqSize {
		self.size
	}
}

impl VirtqPrivate for PackedVq {
	fn create_indirect_ctrl(
		&self,
		send: Option<&[MemDescr]>,
		recv: Option<&[MemDescr]>,
	) -> Result<MemDescr, VirtqError> {
		// Need to match (send, recv) twice, as the "size" of the control descriptor to be pulled must be known in advance.
		let len: usize = match (send, recv) {
			(None, None) => return Err(VirtqError::BufferNotSpecified),
			(None, Some(recv_desc_lst)) => recv_desc_lst.len(),
			(Some(send_desc_lst), None) => send_desc_lst.len(),
			(Some(send_desc_lst), Some(recv_desc_lst)) => send_desc_lst.len() + recv_desc_lst.len(),
		};

		let sz_indrct_lst = match Bytes::new(core::mem::size_of::<pvirtq::Desc>() * len) {
			Some(bytes) => bytes,
			None => return Err(VirtqError::BufferToLarge),
		};

		let ctrl_desc = match MemDescr::pull(sz_indrct_lst) {
			Ok(desc) => desc,
			Err(vq_err) => return Err(vq_err),
		};

		// For indexing into the allocated memory area. This reduces the
		// function to only iterate over the MemDescr once and not twice
		// as otherwise needed if the raw descriptor bytes were to be stored
		// in an array.
		let mut crtl_desc_iter = 0usize;

		let desc_slice = unsafe {
			let size = core::mem::size_of::<pvirtq::Desc>();
			core::slice::from_raw_parts_mut(
				ctrl_desc.ptr as *mut pvirtq::Desc,
				ctrl_desc.len / size,
			)
		};

		match (send, recv) {
			(None, None) => Err(VirtqError::BufferNotSpecified),
			// Only recving descriptorsn (those are writabel by device)
			(None, Some(recv_desc_lst)) => {
				for desc in recv_desc_lst {
					desc_slice[crtl_desc_iter] = pvirtq::Desc {
						addr: paging::virt_to_phys(VirtAddr::from(desc.ptr as u64))
							.as_u64()
							.into(),
						len: (desc.len as u32).into(),
						id: 0.into(),
						flags: virtq::DescF::WRITE,
					};

					crtl_desc_iter += 1;
				}
				Ok(ctrl_desc)
			}
			// Only sending descriptors
			(Some(send_desc_lst), None) => {
				for desc in send_desc_lst {
					desc_slice[crtl_desc_iter] = pvirtq::Desc {
						addr: paging::virt_to_phys(VirtAddr::from(desc.ptr as u64))
							.as_u64()
							.into(),
						len: (desc.len as u32).into(),
						id: 0.into(),
						flags: virtq::DescF::empty(),
					};

					crtl_desc_iter += 1;
				}
				Ok(ctrl_desc)
			}
			(Some(send_desc_lst), Some(recv_desc_lst)) => {
				// Send descriptors ALWAYS before receiving ones.
				for desc in send_desc_lst {
					desc_slice[crtl_desc_iter] = pvirtq::Desc {
						addr: paging::virt_to_phys(VirtAddr::from(desc.ptr as u64))
							.as_u64()
							.into(),
						len: (desc.len as u32).into(),
						id: 0.into(),
						flags: virtq::DescF::empty(),
					};

					crtl_desc_iter += 1;
				}

				for desc in recv_desc_lst {
					desc_slice[crtl_desc_iter] = pvirtq::Desc {
						addr: paging::virt_to_phys(VirtAddr::from(desc.ptr as u64))
							.as_u64()
							.into(),
						len: (desc.len as u32).into(),
						id: 0.into(),
						flags: virtq::DescF::WRITE,
					};

					crtl_desc_iter += 1;
				}

				Ok(ctrl_desc)
			}
		}
	}
}
