//! This module contains Virtio's split virtqueue.
//! See Virito specification v1.1. - 2.6

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::alloc::{Allocator, Layout};
use core::cell::{RefCell, UnsafeCell};
use core::mem::{size_of, MaybeUninit};
use core::{iter, ptr, slice};

use virtio::pci::NotificationData;
use virtio::{le16, virtq};

#[cfg(not(feature = "pci"))]
use super::super::transport::mmio::{ComCfg, NotifCfg, NotifCtrl};
#[cfg(feature = "pci")]
use super::super::transport::pci::{ComCfg, NotifCfg, NotifCtrl};
use super::error::VirtqError;
use super::{
	BuffSpec, BufferToken, BufferType, Bytes, MemDescr, MemPool, TransferToken, Virtq,
	VirtqPrivate, VqIndex, VqSize,
};
use crate::arch::memory_barrier;
use crate::arch::mm::{paging, VirtAddr};
use crate::mm::device_alloc::DeviceAlloc;

// The generic structure eases the creation of the layout for the statically
// sized portion of [AvailRing] and [UsedRing]. This way, to be allocated they
// only need to be extended with the dynamic portion.
#[repr(C)]
struct GenericRing<T: ?Sized> {
	flags: le16,
	idx: le16,

	// Rust does not allow a field other than the last one to be unsized.
	// Unfortunately, this is not the case with the layout in the specification.
	// For this reason, we merge the last two fields and provide appropriate
	// accessor methods.
	ring_and_event: T,
}

type AvailRing = GenericRing<[MaybeUninit<le16>]>;

impl AvailRing {
	fn ring_mut(&mut self) -> &mut [MaybeUninit<le16>] {
		let len = self.ring_and_event.len();
		&mut self.ring_and_event[0..len - 1]
	}
}

// The elements of the unsized field and the last field are not of the same type.
// For this reason, the field stores raw bytes and we have typed accessors.
type UsedRing = GenericRing<[u8]>;

// Used ring is not supposed to be modified by the driver. Thus, we only have _ref methods (and not _mut methods).
impl UsedRing {
	fn ring(&self) -> &[virtq::UsedElem] {
		let ring_len =
			(self.ring_and_event.len() - size_of::<le16>()) / size_of::<virtq::UsedElem>();

		unsafe {
			slice::from_raw_parts(
				ptr::from_ref(&self.ring_and_event).cast::<virtq::UsedElem>(),
				ring_len,
			)
		}
	}
}

struct DescrRing {
	read_idx: u16,
	token_ring: Box<[Option<Box<TransferToken>>]>,
	mem_pool: MemPool,

	/// Descriptor Tables
	///
	/// # Safety
	///
	/// These tables may only be accessed via volatile operations.
	/// See the corresponding method for a safe wrapper.
	descr_table_cell: Box<UnsafeCell<[MaybeUninit<virtq::Desc>]>, DeviceAlloc>,
	avail_ring_cell: Box<UnsafeCell<AvailRing>, DeviceAlloc>,
	used_ring_cell: Box<UnsafeCell<UsedRing>, DeviceAlloc>,
}

impl DescrRing {
	fn descr_table_mut(&mut self) -> &mut [MaybeUninit<virtq::Desc>] {
		unsafe { &mut *self.descr_table_cell.get() }
	}
	fn avail_ring(&self) -> &AvailRing {
		unsafe { &*self.avail_ring_cell.get() }
	}
	fn avail_ring_mut(&mut self) -> &mut AvailRing {
		unsafe { &mut *self.avail_ring_cell.get() }
	}
	fn used_ring(&self) -> &UsedRing {
		unsafe { &*self.used_ring_cell.get() }
	}

	fn push(&mut self, tkn: TransferToken) -> Result<u16, VirtqError> {
		let mut index;
		if let Some(ctrl_desc) = tkn.ctrl_desc.as_ref() {
			let descriptor = virtq::Desc {
				addr: paging::virt_to_phys(VirtAddr::from(ctrl_desc.ptr as u64))
					.as_u64()
					.into(),
				len: (ctrl_desc.len as u32).into(),
				flags: virtq::DescF::INDIRECT,
				next: 0.into(),
			};

			index = self
				.mem_pool
				.pool
				.borrow_mut()
				.pop()
				.ok_or(VirtqError::NoDescrAvail)?
				.0;
			self.descr_table_mut()[usize::from(index)] = MaybeUninit::new(descriptor);
		} else {
			let rev_send_desc_iter = tkn
				.buff_tkn
				.send_buff
				.as_ref()
				.map(|send_buff| send_buff.as_slice().iter())
				.into_iter()
				.flatten()
				.rev()
				.zip(iter::repeat(virtq::DescF::empty()));
			let rev_recv_desc_iter = tkn
				.buff_tkn
				.recv_buff
				.as_ref()
				.map(|recv_buff| recv_buff.as_slice().iter())
				.into_iter()
				.flatten()
				.rev()
				.zip(iter::repeat(virtq::DescF::WRITE));
			let mut rev_all_desc_iter =
				rev_recv_desc_iter
					.chain(rev_send_desc_iter)
					.map(|(mem_descr, flags)| virtq::Desc {
						addr: paging::virt_to_phys(VirtAddr::from(mem_descr.ptr as u64))
							.as_u64()
							.into(),
						len: (mem_descr.len as u32).into(),
						flags,
						next: 0.into(),
					});

			// We need to handle the last descriptor (the first for the reversed iterator) specially to not set the next flag.
			{
				// If the [BufferToken] is empty, we panic
				let descriptor = rev_all_desc_iter.next().unwrap();

				index = self
					.mem_pool
					.pool
					.borrow_mut()
					.pop()
					.ok_or(VirtqError::NoDescrAvail)?
					.0;
				self.descr_table_mut()[usize::from(index)] = MaybeUninit::new(descriptor);
			}
			for mut descriptor in rev_all_desc_iter {
				descriptor.flags |= virtq::DescF::NEXT;
				// We have not updated `index` yet, so it is at this point the index of the previous descriptor that had been written.
				descriptor.next = le16::from(index);

				index = self
					.mem_pool
					.pool
					.borrow_mut()
					.pop()
					.ok_or(VirtqError::NoDescrAvail)?
					.0;
				self.descr_table_mut()[usize::from(index)] = MaybeUninit::new(descriptor);
			}
			// At this point, `index` is the index of the last element of the reversed iterator,
			// thus the head of the descriptor chain.
		}

		self.token_ring[usize::from(index)] = Some(Box::new(tkn));

		let len = self.token_ring.len();
		let idx = self.avail_ring_mut().idx.to_ne();
		self.avail_ring_mut().ring_mut()[idx as usize % len] = MaybeUninit::new(index.into());

		memory_barrier();
		let next_idx = idx.wrapping_add(1);
		self.avail_ring_mut().idx = next_idx.into();

		Ok(next_idx)
	}

	fn poll(&mut self) {
		// We cannot use a simple while loop here because Rust cannot tell that [Self::used_ring_ref],
		// [Self::read_idx] and [Self::token_ring] access separate fields of `self`. For this reason we
		// need to move [Self::used_ring_ref] lines into a separate scope.
		loop {
			let used_elem;
			let cur_ring_index;
			{
				if self.read_idx == self.used_ring().idx.to_ne() {
					break;
				} else {
					cur_ring_index = self.read_idx as usize % self.token_ring.len();
					used_elem = self.used_ring().ring()[cur_ring_index];
				}
			}

			let mut tkn = self.token_ring[used_elem.id.to_ne() as usize]
				.take()
				.expect(
					"The buff_id is incorrect or the reference to the TransferToken was misplaced.",
				);

			if tkn.buff_tkn.recv_buff.as_ref().is_some() {
				tkn.buff_tkn
					.restr_size(None, Some(used_elem.len.to_ne() as usize))
					.unwrap();
			}
			if let Some(queue) = tkn.await_queue.take() {
				queue.try_send(Box::new(tkn.buff_tkn)).unwrap()
			}

			let mut id_ret_idx = u16::try_from(cur_ring_index).unwrap();
			loop {
				self.mem_pool.ret_id(super::MemDescrId(id_ret_idx));
				let cur_chain_elem =
					unsafe { self.descr_table_mut()[usize::from(id_ret_idx)].assume_init() };
				if cur_chain_elem.flags.contains(virtq::DescF::NEXT) {
					id_ret_idx = cur_chain_elem.next.to_ne();
				} else {
					break;
				}
			}

			memory_barrier();
			self.read_idx = self.read_idx.wrapping_add(1);
		}
	}

	fn drv_enable_notif(&mut self) {
		self.avail_ring_mut().flags = 0.into();
	}

	fn drv_disable_notif(&mut self) {
		self.avail_ring_mut().flags = 1.into();
	}

	fn dev_is_notif(&self) -> bool {
		self.used_ring().flags.to_ne() & 1 == 0
	}
}

/// Virtio's split virtqueue structure
pub struct SplitVq {
	ring: RefCell<DescrRing>,
	size: VqSize,
	index: VqIndex,

	notif_ctrl: NotifCtrl,
}

impl Virtq for SplitVq {
	fn enable_notifs(&self) {
		self.ring.borrow_mut().drv_enable_notif();
	}

	fn disable_notifs(&self) {
		self.ring.borrow_mut().drv_disable_notif();
	}

	fn poll(&self) {
		self.ring.borrow_mut().poll()
	}

	fn dispatch_batch(&self, _tkns: Vec<TransferToken>, _notif: bool) -> Result<(), VirtqError> {
		unimplemented!();
	}

	fn dispatch_batch_await(
		&self,
		_tkns: Vec<TransferToken>,
		_await_queue: super::BufferTokenSender,
		_notif: bool,
	) -> Result<(), VirtqError> {
		unimplemented!()
	}

	fn dispatch(&self, tkn: TransferToken, notif: bool) -> Result<(), VirtqError> {
		let next_idx = self.ring.borrow_mut().push(tkn)?;

		if notif {
			// TODO: Check whether the splitvirtquue has notifications for specific descriptors
			// I believe it does not.
			unimplemented!();
		}

		if self.ring.borrow().dev_is_notif() {
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

	fn new(
		com_cfg: &mut ComCfg,
		notif_cfg: &NotifCfg,
		size: VqSize,
		index: VqIndex,
		features: virtio::F,
	) -> Result<Self, VirtqError> {
		// Get a handler to the queues configuration area.
		let mut vq_handler = match com_cfg.select_vq(index.into()) {
			Some(handler) => handler,
			None => return Err(VirtqError::QueueNotExisting(index.into())),
		};

		let size = vq_handler.set_vq_size(size.0);
		const ALLOCATOR: DeviceAlloc = DeviceAlloc;

		let descr_table_cell = unsafe {
			core::mem::transmute::<
				Box<[MaybeUninit<virtq::Desc>], DeviceAlloc>,
				Box<UnsafeCell<[MaybeUninit<virtq::Desc>]>, DeviceAlloc>,
			>(Box::new_uninit_slice_in(size.into(), ALLOCATOR))
		};

		let avail_ring_cell = {
			let ring_and_event_len = usize::from(size) + 1;
			let allocation = ALLOCATOR
				.allocate(
					Layout::new::<GenericRing<()>>() // flags
						.extend(Layout::array::<le16>(ring_and_event_len).unwrap()) // +1 for event
						.unwrap()
						.0
						.pad_to_align(),
				)
				.map_err(|_| VirtqError::AllocationError)?;
			unsafe {
				Box::from_raw_in(
					core::ptr::slice_from_raw_parts_mut(allocation.as_mut_ptr(), ring_and_event_len)
						as *mut UnsafeCell<AvailRing>,
					ALLOCATOR,
				)
			}
		};

		let used_ring_cell = {
			let ring_and_event_layout = Layout::array::<virtq::UsedElem>(size.into())
				.unwrap()
				.extend(Layout::new::<le16>()) // for event
				.unwrap()
				.0;
			let allocation = ALLOCATOR
				.allocate(
					Layout::new::<GenericRing<()>>()
						.extend(ring_and_event_layout)
						.unwrap()
						.0
						.pad_to_align(),
				)
				.map_err(|_| VirtqError::AllocationError)?;
			unsafe {
				Box::from_raw_in(
					core::ptr::slice_from_raw_parts_mut(
						allocation.as_mut_ptr(),
						ring_and_event_layout.size(),
					) as *mut UnsafeCell<UsedRing>,
					ALLOCATOR,
				)
			}
		};

		// Provide memory areas of the queues data structures to the device
		vq_handler.set_ring_addr(paging::virt_to_phys(VirtAddr::from(
			ptr::from_ref(descr_table_cell.as_ref()).expose_provenance(),
		)));
		// As usize is safe here, as the *mut EventSuppr raw pointer is a thin pointer of size usize
		vq_handler.set_drv_ctrl_addr(paging::virt_to_phys(VirtAddr::from(
			ptr::from_ref(avail_ring_cell.as_ref()).expose_provenance(),
		)));
		vq_handler.set_dev_ctrl_addr(paging::virt_to_phys(VirtAddr::from(
			ptr::from_ref(used_ring_cell.as_ref()).expose_provenance(),
		)));

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
			ring: RefCell::new(descr_ring),
			notif_ctrl,
			size: VqSize(size),
			index,
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

impl VirtqPrivate for SplitVq {
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

		let sz_indrct_lst = match Bytes::new(core::mem::size_of::<virtq::Desc>() * len) {
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
		let mut desc_lst_len = len;

		let desc_slice = unsafe {
			let size = core::mem::size_of::<virtq::Desc>();
			core::slice::from_raw_parts_mut(ctrl_desc.ptr as *mut virtq::Desc, ctrl_desc.len / size)
		};

		match (send, recv) {
			(None, None) => Err(VirtqError::BufferNotSpecified),
			// Only recving descriptorsn (those are writabel by device)
			(None, Some(recv_desc_lst)) => {
				for desc in recv_desc_lst {
					desc_slice[crtl_desc_iter] = if desc_lst_len > 1 {
						virtq::Desc {
							addr: paging::virt_to_phys(VirtAddr::from(desc.ptr as u64))
								.as_u64()
								.into(),
							len: (desc.len as u32).into(),
							flags: virtq::DescF::WRITE | virtq::DescF::NEXT,
							next: ((crtl_desc_iter + 1) as u16).into(),
						}
					} else {
						virtq::Desc {
							addr: paging::virt_to_phys(VirtAddr::from(desc.ptr as u64))
								.as_u64()
								.into(),
							len: (desc.len as u32).into(),
							flags: virtq::DescF::WRITE,
							next: 0.into(),
						}
					};

					desc_lst_len -= 1;
					crtl_desc_iter += 1;
				}
				Ok(ctrl_desc)
			}
			// Only sending descriptors
			(Some(send_desc_lst), None) => {
				for desc in send_desc_lst {
					desc_slice[crtl_desc_iter] = if desc_lst_len > 1 {
						virtq::Desc {
							addr: paging::virt_to_phys(VirtAddr::from(desc.ptr as u64))
								.as_u64()
								.into(),
							len: (desc.len as u32).into(),
							flags: virtq::DescF::NEXT,
							next: ((crtl_desc_iter + 1) as u16).into(),
						}
					} else {
						virtq::Desc {
							addr: paging::virt_to_phys(VirtAddr::from(desc.ptr as u64))
								.as_u64()
								.into(),
							len: (desc.len as u32).into(),
							flags: virtq::DescF::empty(),
							next: 0.into(),
						}
					};

					desc_lst_len -= 1;
					crtl_desc_iter += 1;
				}
				Ok(ctrl_desc)
			}
			(Some(send_desc_lst), Some(recv_desc_lst)) => {
				// Send descriptors ALWAYS before receiving ones.
				for desc in send_desc_lst {
					desc_slice[crtl_desc_iter] = if desc_lst_len > 1 {
						virtq::Desc {
							addr: paging::virt_to_phys(VirtAddr::from(desc.ptr as u64))
								.as_u64()
								.into(),
							len: (desc.len as u32).into(),
							flags: virtq::DescF::NEXT,
							next: ((crtl_desc_iter + 1) as u16).into(),
						}
					} else {
						virtq::Desc {
							addr: paging::virt_to_phys(VirtAddr::from(desc.ptr as u64))
								.as_u64()
								.into(),
							len: (desc.len as u32).into(),
							flags: virtq::DescF::empty(),
							next: 0.into(),
						}
					};

					desc_lst_len -= 1;
					crtl_desc_iter += 1;
				}

				for desc in recv_desc_lst {
					desc_slice[crtl_desc_iter] = if desc_lst_len > 1 {
						virtq::Desc {
							addr: paging::virt_to_phys(VirtAddr::from(desc.ptr as u64))
								.as_u64()
								.into(),
							len: (desc.len as u32).into(),
							flags: virtq::DescF::WRITE | virtq::DescF::NEXT,
							next: ((crtl_desc_iter + 1) as u16).into(),
						}
					} else {
						virtq::Desc {
							addr: paging::virt_to_phys(VirtAddr::from(desc.ptr as u64))
								.as_u64()
								.into(),
							len: (desc.len as u32).into(),
							flags: virtq::DescF::WRITE,
							next: 0.into(),
						}
					};

					desc_lst_len -= 1;
					crtl_desc_iter += 1;
				}

				Ok(ctrl_desc)
			}
		}
	}
}
