//! This module contains Virtio's packed virtqueue.
//! See Virito specification v1.1. - 2.7
#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cell::Cell;
use core::sync::atomic::{Ordering, fence};
use core::{ops, ptr};

use align_address::Align;
use memory_addresses::PhysAddr;
#[cfg(not(feature = "pci"))]
use virtio::mmio::NotificationData;
#[cfg(feature = "pci")]
use virtio::pci::NotificationData;
use virtio::pvirtq::{EventSuppressDesc, EventSuppressFlags};
use virtio::virtq::DescF;
use virtio::{RingEventFlags, pvirtq, virtq};

#[cfg(not(feature = "pci"))]
use super::super::transport::mmio::{ComCfg, NotifCfg, NotifCtrl};
#[cfg(feature = "pci")]
use super::super::transport::pci::{ComCfg, NotifCfg, NotifCtrl};
use super::error::VirtqError;
use super::{
	AvailBufferToken, BufferType, MemDescrId, MemPool, TransferToken, UsedBufferToken, Virtq,
	VirtqPrivate, VqIndex, VqSize,
};
use crate::arch::mm::paging::{BasePageSize, PageSize};
use crate::mm::device_alloc::DeviceAlloc;

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
		self.0 = !self.0;
	}
}

/// Structure which allows to control raw ring and operate easily on it
struct DescriptorRing {
	ring: Box<[pvirtq::Desc], DeviceAlloc>,
	tkn_ref_ring: Box<[Option<TransferToken<pvirtq::Desc>>]>,

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
		let ring = unsafe { Box::new_zeroed_slice_in(size.into(), DeviceAlloc).assume_init() };

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
	fn try_recv(&mut self) -> Result<UsedBufferToken, VirtqError> {
		let mut ctrl = self.get_read_ctrler();

		ctrl.poll_next()
			.map(|(tkn, written_len)| {
				UsedBufferToken::from_avail_buffer_token(tkn.buff_tkn, written_len)
			})
			.ok_or(VirtqError::NoNewUsed)
	}

	fn push_batch(
		&mut self,
		tkn_lst: impl IntoIterator<Item = TransferToken<pvirtq::Desc>>,
	) -> Result<RingIdx, VirtqError> {
		// Catch empty push, in order to allow zero initialized first_ctrl_settings struct
		// which will be overwritten in the first iteration of the for-loop

		let first_ctrl_settings;
		let first_buffer;
		let mut ctrl;

		let mut tkn_iterator = tkn_lst.into_iter();
		if let Some(first_tkn) = tkn_iterator.next() {
			ctrl = self.push_without_making_available(&first_tkn)?;
			first_ctrl_settings = (ctrl.start, ctrl.buff_id, ctrl.first_flags);
			first_buffer = first_tkn;
		} else {
			// Empty batches are an error
			return Err(VirtqError::BufferNotSpecified);
		}
		// Push the remaining tokens (if any)
		for tkn in tkn_iterator {
			ctrl.make_avail(tkn);
		}

		// Manually make the first buffer available lastly
		//
		// Providing the first buffer in the list manually
		self.make_avail_with_state(
			first_buffer,
			first_ctrl_settings.0,
			first_ctrl_settings.1,
			first_ctrl_settings.2,
		);
		Ok(RingIdx {
			off: self.write_index,
			wrap: self.drv_wc.0.into(),
		})
	}

	fn push(&mut self, tkn: TransferToken<pvirtq::Desc>) -> Result<RingIdx, VirtqError> {
		self.push_batch([tkn])
	}

	fn push_without_making_available(
		&mut self,
		tkn: &TransferToken<pvirtq::Desc>,
	) -> Result<WriteCtrl<'_>, VirtqError> {
		if tkn.num_consuming_descr() > self.capacity {
			return Err(VirtqError::NoDescrAvail);
		}

		// create an counter that wrappes to the first element
		// after reaching a the end of the ring
		let mut ctrl = self.get_write_ctrler()?;

		// Importance here is:
		// * distinguish between Indirect and direct buffers
		// * make them available in the right order (the first descriptor last) (VIRTIO Spec. v1.2 section 2.8.6)

		// The buffer uses indirect descriptors if the ctrl_desc field is Some.
		if let Some(ctrl_desc) = tkn.ctrl_desc.as_ref() {
			let desc = PackedVq::indirect_desc(ctrl_desc.as_ref());
			ctrl.write_desc(desc);
		} else {
			for incomplete_desc in PackedVq::descriptor_iter(&tkn.buff_tkn)? {
				ctrl.write_desc(incomplete_desc);
			}
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
		let desc_id = self.mem_pool.pool.pop().ok_or(VirtqError::NoDescrAvail)?;
		Ok(WriteCtrl {
			start: self.write_index,
			position: self.write_index,
			modulo: u16::try_from(self.ring.len()).unwrap(),
			first_flags: DescF::empty(),
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
		raw_tkn: TransferToken<pvirtq::Desc>,
		start: u16,
		buff_id: MemDescrId,
		first_flags: DescF,
	) {
		// provide reference, in order to let TransferToken know upon finish.
		self.tkn_ref_ring[usize::from(buff_id.0)] = Some(raw_tkn);
		// The driver performs a suitable memory barrier to ensure the device sees the updated descriptor table and available ring before the next step.
		// See Virtio specfification v1.1. - 2.7.21
		fence(Ordering::SeqCst);
		self.ring[usize::from(start)].flags = first_flags;
	}

	/// Returns the [DescF] with the avail and used flags set in accordance
	/// with the VIRTIO specification v1.2 - 2.8.1 (i.e. avail flag set to match
	/// the driver WrapCount and the used flag set to NOT match the WrapCount).
	///
	/// This function is defined on the whole ring rather than only the
	/// wrap counter to ensure that it is not called on the incorrect
	/// wrap counter (i.e. device wrap counter) by accident.
	///
	/// A copy of the flag is taken instead of a mutable reference
	/// for the cases in which the modification of the flag needs to be
	/// deferred (e.g. patched dispatches, chained buffers).
	fn to_marked_avail(&self, mut flags: DescF) -> DescF {
		flags.set(virtq::DescF::AVAIL, self.drv_wc.0);
		flags.set(virtq::DescF::USED, !self.drv_wc.0);
		flags
	}

	/// Checks the avail and used flags to see if the descriptor is marked
	/// as used by the device in accordance with the
	/// VIRTIO specification v1.2 - 2.8.1 (i.e. they match the device WrapCount)
	///
	/// This function is defined on the whole ring rather than only the
	/// wrap counter to ensure that it is not called on the incorrect
	/// wrap counter (i.e. driver wrap counter) by accident.
	fn is_marked_used(&self, flags: DescF) -> bool {
		if self.dev_wc.0 {
			flags.contains(virtq::DescF::AVAIL | virtq::DescF::USED)
		} else {
			!flags.intersects(virtq::DescF::AVAIL | virtq::DescF::USED)
		}
	}
}

struct ReadCtrl<'a> {
	/// Poll index of the ring at init of ReadCtrl
	position: u16,
	modulo: u16,

	desc_ring: &'a mut DescriptorRing,
}

impl ReadCtrl<'_> {
	/// Polls the ring for a new finished buffer. If buffer is marked as finished, takes care of
	/// updating the queue and returns the respective TransferToken.
	fn poll_next(&mut self) -> Option<(TransferToken<pvirtq::Desc>, u32)> {
		// Check if descriptor has been marked used.
		let desc = &self.desc_ring.ring[usize::from(self.position)];
		if self.desc_ring.is_marked_used(desc.flags) {
			let buff_id = desc.id.to_ne();
			let tkn = self.desc_ring.tkn_ref_ring[usize::from(buff_id)]
				.take()
				.expect(
					"The buff_id is incorrect or the reference to the TransferToken was misplaced.",
				);

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
			// Due to the behavior of the currently used devices and the virtio code from the linux kernel, we assume, that device do NOT set this
			// flag correctly upon writes. Hence we omit it, in order to receive data.

			// We need to read the written length before advancing the position.
			let write_len = desc.len.to_ne();

			for _ in 0..tkn.num_consuming_descr() {
				self.incrmt();
			}
			self.desc_ring.mem_pool.ret_id(MemDescrId(buff_id));

			Some((tkn, write_len))
		} else {
			None
		}
	}

	fn incrmt(&mut self) {
		if self.desc_ring.poll_index + 1 == self.modulo {
			self.desc_ring.dev_wc.wrap();
		}

		// Increment capacity as we have one more free now!
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
	/// The [pvirtq::Desc::flags] value for the first descriptor, the write of which is deferred.
	first_flags: DescF,
	/// Buff ID of this write
	buff_id: MemDescrId,

	desc_ring: &'a mut DescriptorRing,
}

impl WriteCtrl<'_> {
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

	/// Completes the descriptor flags and id, and writes into the queue at the correct position.
	fn write_desc(&mut self, mut incomplete_desc: pvirtq::Desc) {
		incomplete_desc.id = self.buff_id.0.into();
		if self.start == self.position {
			// We save what the flags value for the first descriptor will be to be able
			// to write it later when all the other descriptors are written (so that
			// the device does not see an incomplete chain).
			self.first_flags = self.desc_ring.to_marked_avail(incomplete_desc.flags);
		} else {
			// Set avail and used according to the current WrapCount.
			incomplete_desc.flags = self.desc_ring.to_marked_avail(incomplete_desc.flags);
		}
		self.desc_ring.ring[usize::from(self.position)] = incomplete_desc;
		self.incrmt();
	}

	fn make_avail(&mut self, raw_tkn: TransferToken<pvirtq::Desc>) {
		// We fail if one wants to make a buffer available without inserting one element!
		assert!(self.start != self.position);
		self.desc_ring
			.make_avail_with_state(raw_tkn, self.start, self.buff_id, self.first_flags);
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
	descr_ring: DescriptorRing,
	/// Allows to tell the device if notifications are wanted
	drv_event: DrvNotif,
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
impl Virtq for PackedVq {
	fn enable_notifs(&mut self) {
		self.drv_event.enable_notif();
	}

	fn disable_notifs(&mut self) {
		self.drv_event.disable_notif();
	}

	fn is_empty(&self) -> bool {
		todo!()
	}

	fn try_recv(&mut self) -> Result<UsedBufferToken, VirtqError> {
		self.descr_ring.try_recv()
	}

	fn dispatch_batch(
		&mut self,
		buffer_tkns: Vec<(AvailBufferToken, BufferType)>,
		notif: bool,
	) -> Result<(), VirtqError> {
		// Zero transfers are not allowed
		assert!(!buffer_tkns.is_empty());

		let transfer_tkns = buffer_tkns.into_iter().map(|(buffer_tkn, buffer_type)| {
			Self::transfer_token_from_buffer_token(buffer_tkn, buffer_type)
		});

		let next_idx = self.descr_ring.push_batch(transfer_tkns)?;

		if notif {
			self.drv_event.enable_specific(next_idx);
		}

		let range = self.last_next.get()..next_idx;
		let notif_specific = self
			.dev_event
			.notif_specific()
			.is_some_and(|idx| range.wrapping_contains(&idx));

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
		&mut self,
		buffer_tkns: Vec<(AvailBufferToken, BufferType)>,
		notif: bool,
	) -> Result<(), VirtqError> {
		// Zero transfers are not allowed
		assert!(!buffer_tkns.is_empty());

		let transfer_tkns = buffer_tkns.into_iter().map(|(buffer_tkn, buffer_type)| {
			Self::transfer_token_from_buffer_token(buffer_tkn, buffer_type)
		});

		let next_idx = self.descr_ring.push_batch(transfer_tkns)?;

		if notif {
			self.drv_event.enable_specific(next_idx);
		}

		let range = self.last_next.get()..next_idx;
		let notif_specific = self
			.dev_event
			.notif_specific()
			.is_some_and(|idx| range.wrapping_contains(&idx));

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

	fn dispatch(
		&mut self,
		buffer_tkn: AvailBufferToken,
		notif: bool,
		buffer_type: BufferType,
	) -> Result<(), VirtqError> {
		let transfer_tkn = Self::transfer_token_from_buffer_token(buffer_tkn, buffer_type);
		let next_idx = self.descr_ring.push(transfer_tkn)?;

		if notif {
			self.drv_event.enable_specific(next_idx);
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

	fn size(&self) -> VqSize {
		self.size
	}

	fn has_used_buffers(&self) -> bool {
		let desc = &self.descr_ring.ring[usize::from(self.descr_ring.poll_index)];
		self.descr_ring.is_marked_used(desc.flags)
	}
}

impl VirtqPrivate for PackedVq {
	type Descriptor = pvirtq::Desc;

	fn create_indirect_ctrl(
		buffer_tkn: &AvailBufferToken,
	) -> Result<Box<[Self::Descriptor]>, VirtqError> {
		Ok(Self::descriptor_iter(buffer_tkn)?
			.collect::<Vec<_>>()
			.into_boxed_slice())
	}
}

impl PackedVq {
	pub(crate) fn new(
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
		let Some(mut vq_handler) = com_cfg.select_vq(index.into()) else {
			return Err(VirtqError::QueueNotExisting(index.into()));
		};

		// Must catch zero size as it is not allowed for packed queues.
		// Must catch size larger 0x8000 (2^15) as it is not allowed for packed queues.
		//
		// See Virtio specification v1.1. - 4.1.4.3.2
		let vq_size = if (size.0 == 0) | (size.0 > 0x8000) {
			return Err(VirtqError::QueueSizeNotAllowed(size.0));
		} else {
			vq_handler.set_vq_size(size.0)
		};

		let descr_ring = DescriptorRing::new(vq_size);
		// Allocate heap memory via a vec, leak and cast
		let _mem_len =
			core::mem::size_of::<pvirtq::EventSuppress>().align_up(BasePageSize::SIZE as usize);

		let drv_event = Box::<pvirtq::EventSuppress, _>::new_zeroed_in(DeviceAlloc);
		let dev_event = Box::<pvirtq::EventSuppress, _>::new_zeroed_in(DeviceAlloc);
		// TODO: make this safe using zerocopy
		let drv_event = unsafe { drv_event.assume_init() };
		let dev_event = unsafe { dev_event.assume_init() };
		let drv_event = Box::leak(drv_event);
		let dev_event = Box::leak(dev_event);

		// Provide memory areas of the queues data structures to the device
		vq_handler.set_ring_addr(PhysAddr::from(descr_ring.raw_addr()));
		// As usize is safe here, as the *mut EventSuppr raw pointer is a thin pointer of size usize
		vq_handler.set_drv_ctrl_addr(PhysAddr::from(ptr::from_mut(drv_event).expose_provenance()));
		vq_handler.set_dev_ctrl_addr(PhysAddr::from(ptr::from_mut(dev_event).expose_provenance()));

		let mut drv_event = DrvNotif {
			f_notif_idx: false,
			raw: drv_event,
		};

		let dev_event = DevNotif {
			f_notif_idx: false,
			raw: dev_event,
		};

		let mut notif_ctrl = NotifCtrl::new(notif_cfg.notification_location(&mut vq_handler));

		if features.contains(virtio::F::NOTIFICATION_DATA) {
			notif_ctrl.enable_notif_data();
		}

		if features.contains(virtio::F::EVENT_IDX) {
			drv_event.f_notif_idx = true;
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
			last_next: Cell::default(),
		})
	}
}
