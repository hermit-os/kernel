//! This module contains Virtio's split virtqueue.
//! See Virito specification v1.1. - 2.6

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::mem::{self, MaybeUninit};
use core::ptr;

use memory_addresses::PhysAddr;
#[cfg(not(feature = "pci"))]
use virtio::mmio::NotificationData;
#[cfg(feature = "pci")]
use virtio::pci::NotificationData;
use virtio::{le16, virtq};

#[cfg(not(feature = "pci"))]
use super::super::transport::mmio::{ComCfg, NotifCfg, NotifCtrl};
#[cfg(feature = "pci")]
use super::super::transport::pci::{ComCfg, NotifCfg, NotifCtrl};
use super::error::VirtqError;
use super::{
	AvailBufferToken, BufferType, MemPool, TransferToken, UsedBufferToken, Virtq, VirtqPrivate,
	VqIndex, VqSize,
};
use crate::arch::memory_barrier;
use crate::mm::device_alloc::DeviceAlloc;

struct DescrRing {
	read_idx: u16,
	token_ring: Box<[Option<TransferToken<virtq::Desc>>]>,
	mem_pool: MemPool,

	/// Descriptor Tables
	///
	/// # Safety
	///
	/// These tables may only be accessed via volatile operations.
	/// See the corresponding method for a safe wrapper.
	descr_table_cell: Box<UnsafeCell<[MaybeUninit<virtq::Desc>]>, DeviceAlloc>,
	avail_ring_cell: Box<UnsafeCell<virtq::Avail>, DeviceAlloc>,
	used_ring_cell: Box<UnsafeCell<virtq::Used>, DeviceAlloc>,
}

impl DescrRing {
	fn descr_table_mut(&mut self) -> &mut [MaybeUninit<virtq::Desc>] {
		unsafe { &mut *self.descr_table_cell.get() }
	}
	fn avail_ring(&self) -> &virtq::Avail {
		unsafe { &*self.avail_ring_cell.get() }
	}
	fn avail_ring_mut(&mut self) -> &mut virtq::Avail {
		unsafe { &mut *self.avail_ring_cell.get() }
	}
	fn used_ring(&self) -> &virtq::Used {
		unsafe { &*self.used_ring_cell.get() }
	}

	fn is_empty(&self) -> bool {
		self.mem_pool.all_available()
	}

	fn push(&mut self, tkn: TransferToken<virtq::Desc>) -> Result<u16, VirtqError> {
		let mut index;
		if let Some(ctrl_desc) = tkn.ctrl_desc.as_ref() {
			trace!("<vq:split> Creating indirect descriptor");
			let descriptor = SplitVq::indirect_desc(ctrl_desc.as_ref());

			trace!("<vq:split> Attempting to assign descriptor to free slot in table");

			index = self.mem_pool.pool.pop().ok_or(VirtqError::NoDescrAvail)?.0;
			trace!("<vq:split> Assigned one descriptor (indirect)");
			self.descr_table_mut()[usize::from(index)] = MaybeUninit::new(descriptor);
		} else {
			trace!("<vq:split> Creating direct descriptor iterator");
			let mut rev_all_desc_iter = SplitVq::descriptor_iter(&tkn.buff_tkn)?.rev();

			trace!(
				"<vq:split> Attempting to assign descriptors to free slots in table in reverse order"
			);

			let mut num_descriptors_assigned = 0;

			// We need to handle the last descriptor (the first for the reversed iterator) specially to not set the next flag.
			{
				// If the [AvailBufferToken] is empty, we panic
				let descriptor = rev_all_desc_iter.next().unwrap();

				index = self.mem_pool.pool.pop().ok_or(VirtqError::NoDescrAvail)?.0;
				num_descriptors_assigned += 1;
				self.descr_table_mut()[usize::from(index)] = MaybeUninit::new(descriptor);
			}
			for mut descriptor in rev_all_desc_iter {
				// We have not updated `index` yet, so it is at this point the index of the previous descriptor that had been written.
				descriptor.next = le16::from(index);

				index = self.mem_pool.pool.pop().ok_or(VirtqError::NoDescrAvail)?.0;
				num_descriptors_assigned += 1;
				self.descr_table_mut()[usize::from(index)] = MaybeUninit::new(descriptor);
			}
			// At this point, `index` is the index of the last element of the reversed iterator,
			// thus the head of the descriptor chain.
			trace!("<vq:split> Assigned {num_descriptors_assigned} descriptors (direct)");
		}

		trace!("<vq:split> Inserting transfer token into token ring at index {index}");

		self.token_ring[usize::from(index)] = Some(tkn);

		trace!("<vq:split> Updating available ring");

		let len = self.token_ring.len();
		let idx = self.avail_ring_mut().idx.to_ne();
		self.avail_ring_mut().ring_mut(true)[idx as usize % len] = index.into();

		memory_barrier();
		let next_idx = idx.wrapping_add(1);
		self.avail_ring_mut().idx = next_idx.into();

		Ok(next_idx)
	}

	fn try_recv(&mut self) -> Result<UsedBufferToken, VirtqError> {
		if self.read_idx == self.used_ring().idx.to_ne() {
			return Err(VirtqError::NoNewUsed);
		}
		let cur_ring_index = self.read_idx as usize % self.token_ring.len();
		let used_elem = self.used_ring().ring()[cur_ring_index];

		let tkn = self.token_ring[used_elem.id.to_ne() as usize]
			.take()
			.expect(
				"The buff_id is incorrect or the reference to the TransferToken was misplaced.",
			);

		let mut num_descriptors_freed = 0;
		// We return the indices of the now freed ring slots back to `mem_pool.`
		let mut id_ret_idx = u16::try_from(used_elem.id.to_ne()).unwrap();
		loop {
			num_descriptors_freed += 1;
			self.mem_pool.ret_id(super::MemDescrId(id_ret_idx));
			let cur_chain_elem =
				unsafe { self.descr_table_mut()[usize::from(id_ret_idx)].assume_init() };
			if cur_chain_elem.flags.contains(virtq::DescF::NEXT) {
				id_ret_idx = cur_chain_elem.next.to_ne();
			} else {
				break;
			}
		}
		trace!("<vq:split> freed {num_descriptors_freed} descriptors");

		memory_barrier();
		self.read_idx = self.read_idx.wrapping_add(1);
		Ok(UsedBufferToken::from_avail_buffer_token(
			tkn.buff_tkn,
			used_elem.len.to_ne(),
		))
	}

	fn drv_enable_notif(&mut self) {
		self.avail_ring_mut()
			.flags
			.remove(virtq::AvailF::NO_INTERRUPT);
	}

	fn drv_disable_notif(&mut self) {
		self.avail_ring_mut()
			.flags
			.insert(virtq::AvailF::NO_INTERRUPT);
	}

	fn dev_is_notif(&self) -> bool {
		!self.used_ring().flags.contains(virtq::UsedF::NO_NOTIFY)
	}
}

/// Virtio's split virtqueue structure
pub struct SplitVq {
	ring: DescrRing,
	size: VqSize,
	index: VqIndex,

	notif_ctrl: NotifCtrl,
}

impl Virtq for SplitVq {
	fn enable_notifs(&mut self) {
		self.ring.drv_enable_notif();
	}

	fn disable_notifs(&mut self) {
		self.ring.drv_disable_notif();
	}

	fn try_recv(&mut self) -> Result<UsedBufferToken, VirtqError> {
		self.ring.try_recv()
	}

	fn is_empty(&self) -> bool {
		self.ring.is_empty()
	}

	fn dispatch_batch(
		&mut self,
		_tkns: Vec<(AvailBufferToken, BufferType)>,
		_notif: bool,
	) -> Result<(), VirtqError> {
		unimplemented!();
	}

	fn dispatch_batch_await(
		&mut self,
		_tkns: Vec<(AvailBufferToken, BufferType)>,
		_notif: bool,
	) -> Result<(), VirtqError> {
		unimplemented!()
	}

	fn dispatch(
		&mut self,
		buffer_tkn: AvailBufferToken,
		notif: bool,
		buffer_type: BufferType,
	) -> Result<(), VirtqError> {
		// IMPORTANT: This function may not allocate from GlobalAlloc if the
		//            balloon device support has been enabled, as the inflate/deflate
		//            operations operate with a locked global allocator and need
		//            to send descriptors into their respective queues.
		//            Allocating with the global allocator here would deadlock.

		trace!("<vq:split> Creating transfer token");
		let transfer_tkn = Self::transfer_token_from_buffer_token(buffer_tkn, buffer_type);
		trace!("<vq:split> Pushing to descriptor ring transfer token");
		let next_idx = self.ring.push(transfer_tkn)?;

		if notif {
			// TODO: Check whether the splitvirtquue has notifications for specific descriptors
			// I believe it does not.
			unimplemented!();
		}

		if self.ring.dev_is_notif() {
			let notification_data = NotificationData::new()
				.with_vqn(self.index.0)
				.with_next_idx(next_idx);
			self.notif_ctrl.notify_dev(notification_data);
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
		self.ring.read_idx != self.ring.used_ring().idx.to_ne()
	}
}

impl VirtqPrivate for SplitVq {
	type Descriptor = virtq::Desc;
	fn create_indirect_ctrl(
		buffer_tkn: &AvailBufferToken,
	) -> Result<Box<[Self::Descriptor]>, VirtqError> {
		Ok(Self::descriptor_iter(buffer_tkn)?
			.zip(1..)
			.map(|(descriptor, next_id)| Self::Descriptor {
				next: next_id.into(),
				..descriptor
			})
			.collect::<Vec<_>>()
			.into_boxed_slice())
	}
}

impl SplitVq {
	pub(crate) fn new(
		com_cfg: &mut ComCfg,
		notif_cfg: &NotifCfg,
		size: VqSize,
		index: VqIndex,
		features: virtio::F,
	) -> Result<Self, VirtqError> {
		// Get a handler to the queues configuration area.
		let Some(mut vq_handler) = com_cfg.select_vq(index.into()) else {
			return Err(VirtqError::QueueNotExisting(index.into()));
		};

		let size = vq_handler.set_vq_size(size.0);

		let descr_table_cell = unsafe {
			core::mem::transmute::<
				Box<[MaybeUninit<virtq::Desc>], DeviceAlloc>,
				Box<UnsafeCell<[MaybeUninit<virtq::Desc>]>, DeviceAlloc>,
			>(Box::new_uninit_slice_in(size.into(), DeviceAlloc))
		};

		let avail_ring_cell = {
			let avail = virtq::Avail::try_new_in(size, true, DeviceAlloc)
				.map_err(|_| VirtqError::AllocationError)?;

			unsafe {
				mem::transmute::<
					Box<virtq::Avail, DeviceAlloc>,
					Box<UnsafeCell<virtq::Avail>, DeviceAlloc>,
				>(avail)
			}
		};

		let used_ring_cell = {
			let used = virtq::Used::try_new_in(size, true, DeviceAlloc)
				.map_err(|_| VirtqError::AllocationError)?;

			unsafe {
				mem::transmute::<
					Box<virtq::Used, DeviceAlloc>,
					Box<UnsafeCell<virtq::Used>, DeviceAlloc>,
				>(used)
			}
		};

		// Provide memory areas of the queues data structures to the device
		vq_handler.set_ring_addr(PhysAddr::from(
			ptr::from_ref(descr_table_cell.as_ref()).expose_provenance(),
		));
		// As usize is safe here, as the *mut EventSuppr raw pointer is a thin pointer of size usize
		vq_handler.set_drv_ctrl_addr(PhysAddr::from(
			ptr::from_ref(avail_ring_cell.as_ref()).expose_provenance(),
		));
		vq_handler.set_dev_ctrl_addr(PhysAddr::from(
			ptr::from_ref(used_ring_cell.as_ref()).expose_provenance(),
		));

		let descr_ring = DescrRing {
			read_idx: 0,
			token_ring: core::iter::repeat_with(|| None)
				.take(size.into())
				.collect::<Vec<_>>()
				.into_boxed_slice(),
			mem_pool: MemPool::new(size),

			descr_table_cell,
			avail_ring_cell,
			used_ring_cell,
		};

		let mut notif_ctrl = NotifCtrl::new(notif_cfg.notification_location(&mut vq_handler));

		if features.contains(virtio::F::NOTIFICATION_DATA) {
			notif_ctrl.enable_notif_data();
		}

		vq_handler.enable_queue();

		info!("Created SplitVq: idx={}, size={}", index.0, size);

		Ok(SplitVq {
			ring: descr_ring,
			notif_ctrl,
			size: VqSize(size),
			index,
		})
	}
}
