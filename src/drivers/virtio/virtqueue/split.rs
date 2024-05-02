//! This module contains Virtio's split virtqueue.
//! See Virito specification v1.1. - 2.6
#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::alloc::{Allocator, Layout};
use core::cell::RefCell;
use core::mem::{size_of, MaybeUninit};
use core::ptr;

use zerocopy::{little_endian, FromBytes, FromZeroes};

#[cfg(not(feature = "pci"))]
use super::super::transport::mmio::{ComCfg, NotifCfg, NotifCtrl};
#[cfg(feature = "pci")]
use super::super::transport::pci::{ComCfg, NotifCfg, NotifCtrl};
use super::error::VirtqError;
use super::{
	BuffSpec, BufferToken, Bytes, DescrFlags, MemDescr, MemPool, Transfer, TransferState,
	TransferToken, Virtq, VirtqPrivate, VqIndex, VqSize,
};
use crate::arch::memory_barrier;
use crate::arch::mm::{paging, VirtAddr};
use crate::mm::device_alloc::DeviceAlloc;

#[repr(C)]
#[derive(Copy, Clone)]
struct Descriptor {
	address: little_endian::U64,
	len: little_endian::U32,
	flags: little_endian::U16,
	next: little_endian::U16,
}

impl Descriptor {
	fn new(addr: u64, len: u32, flags: u16, next: u16) -> Self {
		Descriptor {
			address: addr.into(),
			len: len.into(),
			flags: flags.into(),
			next: next.into(),
		}
	}
}

// The generic structure eases the creation of the layout for the statically
// sized portion of [AvailRing] and [UsedRing]. This way, to be allocated they
// only need to be extended with the dynamic portion.
#[repr(C)]
struct GenericRing<T: ?Sized> {
	flags: little_endian::U16,
	index: little_endian::U16,

	// Rust does not allow a field other than the last one to be unsized.
	// Unfortunately, this is not the case with the layout in the specification.
	// For this reason, we merge the last two fields and provide appropriate
	// accessor methods.
	ring_and_event: T,
}

const RING_AND_EVENT_ERROR: &str = "ring_and_event should have at least enough elements for the event. It seems to be allocated incorrectly.";

type AvailRing = GenericRing<[MaybeUninit<little_endian::U16>]>;

impl AvailRing {
	fn ring_ref(&self) -> &[MaybeUninit<little_endian::U16>] {
		self.ring_and_event
			.split_last()
			.expect(RING_AND_EVENT_ERROR)
			.1
	}

	fn ring_mut(&mut self) -> &mut [MaybeUninit<little_endian::U16>] {
		self.ring_and_event
			.split_last_mut()
			.expect(RING_AND_EVENT_ERROR)
			.1
	}

	fn event_ref(&self) -> &MaybeUninit<little_endian::U16> {
		self.ring_and_event.last().expect(RING_AND_EVENT_ERROR)
	}

	fn event_mut(&mut self) -> &MaybeUninit<little_endian::U16> {
		self.ring_and_event.last_mut().expect(RING_AND_EVENT_ERROR)
	}
}

// The elements of the unsized field and the last field are not of the same type.
// For this reason, the field stores raw bytes and we have typed accessors.
type UsedRing = GenericRing<[u8]>;

// Used ring is not supposed to be modified by the driver. Thus, we only have _ref methods (and not _mut methods).
impl UsedRing {
	fn ring_ref(&self) -> &[UsedElem] {
		// The last two bytes belong to the event field
		UsedElem::slice_from(
			&self.ring_and_event[..(self.ring_and_event.len() - size_of::<little_endian::U16>())],
		)
		.expect(RING_AND_EVENT_ERROR)
	}

	fn event_ref(&self) -> &little_endian::U16 {
		little_endian::U16::ref_from_suffix(&self.ring_and_event).expect(RING_AND_EVENT_ERROR)
	}
}

#[repr(C)]
#[derive(Copy, Clone, FromZeroes, FromBytes)]
struct UsedElem {
	id: little_endian::U32,
	len: little_endian::U32,
}

struct DescrRing {
	read_idx: u16,
	descr_table: Box<[MaybeUninit<Descriptor>], DeviceAlloc>,
	ref_ring: Box<[Option<Box<TransferToken>>]>,
	avail_ring: Box<AvailRing, DeviceAlloc>,
	used_ring: Box<UsedRing, DeviceAlloc>,
}

impl DescrRing {
	fn push(&mut self, tkn: TransferToken) -> (u16, u16) {
		let mut desc_lst = Vec::new();
		let mut is_indirect = false;

		if let Some(buff) = tkn.buff_tkn.as_ref().unwrap().send_buff.as_ref() {
			if buff.is_indirect() {
				desc_lst.push((buff.get_ctrl_desc().unwrap(), false));
				is_indirect = true;
			} else {
				for desc in buff.as_slice() {
					desc_lst.push((desc, false));
				}
			}
		}

		if let Some(buff) = tkn.buff_tkn.as_ref().unwrap().recv_buff.as_ref() {
			if buff.is_indirect() {
				if desc_lst.is_empty() {
					desc_lst.push((buff.get_ctrl_desc().unwrap(), true));
					is_indirect = true;
				} else if desc_lst.len() == 1 {
					//ensure write flag is set
					let (_, is_write) = &mut desc_lst[0];
					*is_write = true;
				} else {
					panic!("Indirect descriptor should always be inserted as a single descriptor in the queue...")
				}
			} else {
				for desc in buff.as_slice() {
					desc_lst.push((desc, true));
				}
			}
		}

		let mut len = tkn.buff_tkn.as_ref().unwrap().num_consuming_descr();

		assert!(!desc_lst.is_empty());
		// Minus 1, comes from  the fact that ids run from one to 255 and not from 0 to 254 for u8::MAX sized pool
		let index = {
			let (desc, _) = desc_lst[0];
			(desc.id.as_ref().unwrap().0 - 1) as usize
		};
		let mut desc_cnt = 0usize;

		while len != 0 {
			let (desc, is_write) = desc_lst[desc_cnt];
			// This is due to dhe fact that i have ids from one to 255 and not from 0 to 254 for u8::MAX sized pool
			let write_indx = (desc.id.as_ref().unwrap().0 - 1) as usize;

			let descriptor = if is_indirect {
				assert!(len == 1);
				if is_write {
					Descriptor::new(
						paging::virt_to_phys(VirtAddr::from(desc.ptr as u64)).into(),
						desc.len as u32,
						DescrFlags::VIRTQ_DESC_F_INDIRECT | DescrFlags::VIRTQ_DESC_F_WRITE,
						0,
					)
				} else {
					Descriptor::new(
						paging::virt_to_phys(VirtAddr::from(desc.ptr as u64)).into(),
						desc.len as u32,
						DescrFlags::VIRTQ_DESC_F_INDIRECT.into(),
						0,
					)
				}
			} else if len > 1 {
				let next_index = {
					let (desc, _) = desc_lst[desc_cnt + 1];
					desc.id.as_ref().unwrap().0 - 1
				};

				if is_write {
					Descriptor::new(
						paging::virt_to_phys(VirtAddr::from(desc.ptr as u64)).into(),
						desc.len as u32,
						DescrFlags::VIRTQ_DESC_F_WRITE | DescrFlags::VIRTQ_DESC_F_NEXT,
						next_index,
					)
				} else {
					Descriptor::new(
						paging::virt_to_phys(VirtAddr::from(desc.ptr as u64)).into(),
						desc.len as u32,
						DescrFlags::VIRTQ_DESC_F_NEXT.into(),
						next_index,
					)
				}
			} else if is_write {
				Descriptor::new(
					paging::virt_to_phys(VirtAddr::from(desc.ptr as u64)).into(),
					desc.len as u32,
					DescrFlags::VIRTQ_DESC_F_WRITE.into(),
					0,
				)
			} else {
				Descriptor::new(
					paging::virt_to_phys(VirtAddr::from(desc.ptr as u64)).into(),
					desc.len as u32,
					0,
					0,
				)
			};

			self.descr_table[write_indx] = MaybeUninit::new(descriptor);

			desc_cnt += 1;
			len -= 1;
		}

		self.ref_ring[index] = Some(Box::new(tkn));
		let idx = self.avail_ring.index.get();
		let len = self.avail_ring.ring_ref().len();
		self.avail_ring.ring_mut()[idx as usize % len] = MaybeUninit::new((index as u16).into());

		memory_barrier();
		self.avail_ring.index = (self.avail_ring.index.get().wrapping_add(1)).into();

		(0, 0)
	}

	fn poll(&mut self) {
		while self.read_idx
			!= unsafe { ptr::read_volatile(ptr::addr_of!(self.used_ring.index)).get() }
		{
			let cur_ring_index = self.read_idx as usize % self.used_ring.ring_ref().len();
			let used_elem =
				unsafe { ptr::read_volatile(&self.used_ring.ring_ref()[cur_ring_index]) };

			let mut tkn = self.ref_ring[used_elem.id.get() as usize].take().expect(
				"The buff_id is incorrect or the reference to the TransferToken was misplaced.",
			);

			if tkn.buff_tkn.as_ref().unwrap().recv_buff.as_ref().is_some() {
				tkn.buff_tkn
					.as_mut()
					.unwrap()
					.restr_size(None, Some(used_elem.len.get() as usize))
					.unwrap();
			}
			tkn.state = TransferState::Finished;
			if let Some(queue) = tkn.await_queue.take() {
				queue.borrow_mut().push_back(Transfer {
					transfer_tkn: Some(tkn),
				})
			}
			memory_barrier();
			self.read_idx = self.read_idx.wrapping_add(1);
		}
	}

	fn drv_enable_notif(&mut self) {
		self.avail_ring.flags = 0.into();
	}

	fn drv_disable_notif(&mut self) {
		self.avail_ring.flags = 1.into();
	}

	fn dev_is_notif(&self) -> bool {
		self.used_ring.flags & 1.into() == little_endian::U16::new(0)
	}
}

/// Virtio's split virtqueue structure
pub struct SplitVq {
	ring: RefCell<DescrRing>,
	mem_pool: Rc<MemPool>,
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

	fn dispatch_batch(&self, _tkns: Vec<TransferToken>, _notif: bool) {
		unimplemented!();
	}

	fn dispatch_batch_await(
		&self,
		_tkns: Vec<TransferToken>,
		_await_queue: Rc<RefCell<VecDeque<Transfer>>>,
		_notif: bool,
	) {
		unimplemented!()
	}

	fn dispatch(&self, tkn: TransferToken, notif: bool) {
		let (next_off, next_wrap) = self.ring.borrow_mut().push(tkn);

		if notif {
			// TODO: Check whether the splitvirtquue has notifications for specific descriptors
			// I believe it does not.
			unimplemented!();
		}

		if self.ring.borrow().dev_is_notif() {
			let index = self.index.0.to_le_bytes();
			let mut index = index.iter();
			// Even on 64bit systems this is fine, as we have a queue_size < 2^15!
			let det_notif_data: u16 = next_off >> 1;
			let flags = (det_notif_data | (next_wrap << 15)).to_le_bytes();
			let mut flags = flags.iter();
			let mut notif_data: [u8; 4] = [0, 0, 0, 0];

			for (i, byte) in notif_data.iter_mut().enumerate() {
				if i < 2 {
					*byte = *index.next().unwrap();
				} else {
					*byte = *flags.next().unwrap();
				}
			}

			self.notif_ctrl.notify_dev(&notif_data)
		}
	}

	fn index(&self) -> VqIndex {
		self.index
	}

	fn new(
		com_cfg: &mut ComCfg,
		notif_cfg: &NotifCfg,
		size: VqSize,
		index: VqIndex,
		_feats: u64,
	) -> Result<Self, VirtqError> {
		// Get a handler to the queues configuration area.
		let mut vq_handler = match com_cfg.select_vq(index.into()) {
			Some(handler) => handler,
			None => return Err(VirtqError::QueueNotExisting(index.into())),
		};

		let size = vq_handler.set_vq_size(size.0);
		const ALLOCATOR: DeviceAlloc = DeviceAlloc;

		// Allocate heap memory via a vec, leak and cast
		let descr_table = Box::new_uninit_slice_in(size.into(), ALLOCATOR);

		let avail_ring = {
			let ring_and_event_len = usize::from(size) + 1;
			let allocation = ALLOCATOR
				.allocate(
					Layout::new::<GenericRing<()>>() // flags
						.extend(Layout::array::<little_endian::U16>(ring_and_event_len).unwrap()) // +1 for event
						.unwrap()
						.0
						.pad_to_align(),
				)
				.map_err(|_| VirtqError::AllocationError)?;
			unsafe {
				Box::from_raw_in(
					core::ptr::slice_from_raw_parts_mut(allocation.as_mut_ptr(), ring_and_event_len)
						as *mut AvailRing,
					ALLOCATOR,
				)
			}
		};

		let used_ring = {
			let ring_and_event_layout = Layout::array::<UsedElem>(size.into())
				.unwrap()
				.extend(Layout::new::<little_endian::U16>()) // for event
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
					) as *mut UsedRing,
					ALLOCATOR,
				)
			}
		};

		// Provide memory areas of the queues data structures to the device
		vq_handler.set_ring_addr(paging::virt_to_phys(VirtAddr::from(
			descr_table.as_ptr().expose_provenance(),
		)));
		// As usize is safe here, as the *mut EventSuppr raw pointer is a thin pointer of size usize
		vq_handler.set_drv_ctrl_addr(paging::virt_to_phys(VirtAddr::from(
			ptr::from_ref(avail_ring.as_ref()).expose_provenance(),
		)));
		vq_handler.set_dev_ctrl_addr(paging::virt_to_phys(VirtAddr::from(
			ptr::from_ref(used_ring.as_ref()).expose_provenance(),
		)));

		let descr_ring = DescrRing {
			read_idx: 0,
			ref_ring: core::iter::repeat_with(|| None)
				.take(size.into())
				.collect::<Vec<_>>()
				.into_boxed_slice(),
			descr_table,
			avail_ring,
			used_ring,
		};

		let notif_ctrl = NotifCtrl::new(ptr::with_exposed_provenance_mut(
			notif_cfg.base()
				+ usize::from(vq_handler.notif_off())
				+ usize::try_from(notif_cfg.multiplier()).unwrap(),
		));

		// Initialize new memory pool.
		let mem_pool = Rc::new(MemPool::new(size));

		vq_handler.enable_queue();

		info!("Created SplitVq: idx={}, size={}", index.0, size);

		Ok(SplitVq {
			ring: RefCell::new(descr_ring),
			notif_ctrl,
			mem_pool,
			size: VqSize(size),
			index,
		})
	}

	fn prep_transfer_from_raw(
		self: Rc<Self>,
		send: Option<(&[u8], BuffSpec<'_>)>,
		recv: Option<(&mut [u8], BuffSpec<'_>)>,
	) -> Result<TransferToken, VirtqError> {
		self.prep_transfer_from_raw_static(send, recv)
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
		send: Option<&Vec<MemDescr>>,
		recv: Option<&Vec<MemDescr>>,
	) -> Result<MemDescr, VirtqError> {
		// Need to match (send, recv) twice, as the "size" of the control descriptor to be pulled must be known in advance.
		let len: usize = match (send, recv) {
			(None, None) => return Err(VirtqError::BufferNotSpecified),
			(None, Some(recv_desc_lst)) => recv_desc_lst.len(),
			(Some(send_desc_lst), None) => send_desc_lst.len(),
			(Some(send_desc_lst), Some(recv_desc_lst)) => send_desc_lst.len() + recv_desc_lst.len(),
		};

		let sz_indrct_lst = match Bytes::new(core::mem::size_of::<Descriptor>() * len) {
			Some(bytes) => bytes,
			None => return Err(VirtqError::BufferToLarge),
		};

		let ctrl_desc = match self.mem_pool.pull(Rc::clone(&self.mem_pool), sz_indrct_lst) {
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
			let size = core::mem::size_of::<Descriptor>();
			core::slice::from_raw_parts_mut(ctrl_desc.ptr as *mut Descriptor, ctrl_desc.len / size)
		};

		match (send, recv) {
			(None, None) => Err(VirtqError::BufferNotSpecified),
			// Only recving descriptorsn (those are writabel by device)
			(None, Some(recv_desc_lst)) => {
				for desc in recv_desc_lst {
					desc_slice[crtl_desc_iter] = if desc_lst_len > 1 {
						Descriptor::new(
							paging::virt_to_phys(VirtAddr::from(desc.ptr as u64)).into(),
							desc.len as u32,
							DescrFlags::VIRTQ_DESC_F_WRITE | DescrFlags::VIRTQ_DESC_F_NEXT,
							(crtl_desc_iter + 1) as u16,
						)
					} else {
						Descriptor::new(
							paging::virt_to_phys(VirtAddr::from(desc.ptr as u64)).into(),
							desc.len as u32,
							DescrFlags::VIRTQ_DESC_F_WRITE.into(),
							0,
						)
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
						Descriptor::new(
							paging::virt_to_phys(VirtAddr::from(desc.ptr as u64)).into(),
							desc.len as u32,
							DescrFlags::VIRTQ_DESC_F_NEXT.into(),
							(crtl_desc_iter + 1) as u16,
						)
					} else {
						Descriptor::new(
							paging::virt_to_phys(VirtAddr::from(desc.ptr as u64)).into(),
							desc.len as u32,
							0,
							0,
						)
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
						Descriptor::new(
							paging::virt_to_phys(VirtAddr::from(desc.ptr as u64)).into(),
							desc.len as u32,
							DescrFlags::VIRTQ_DESC_F_NEXT.into(),
							(crtl_desc_iter + 1) as u16,
						)
					} else {
						Descriptor::new(
							paging::virt_to_phys(VirtAddr::from(desc.ptr as u64)).into(),
							desc.len as u32,
							0,
							0,
						)
					};

					desc_lst_len -= 1;
					crtl_desc_iter += 1;
				}

				for desc in recv_desc_lst {
					desc_slice[crtl_desc_iter] = if desc_lst_len > 1 {
						Descriptor::new(
							paging::virt_to_phys(VirtAddr::from(desc.ptr as u64)).into(),
							desc.len as u32,
							DescrFlags::VIRTQ_DESC_F_WRITE | DescrFlags::VIRTQ_DESC_F_NEXT,
							(crtl_desc_iter + 1) as u16,
						)
					} else {
						Descriptor::new(
							paging::virt_to_phys(VirtAddr::from(desc.ptr as u64)).into(),
							desc.len as u32,
							DescrFlags::VIRTQ_DESC_F_WRITE.into(),
							0,
						)
					};

					desc_lst_len -= 1;
					crtl_desc_iter += 1;
				}

				Ok(ctrl_desc)
			}
		}
	}
	fn mem_pool(&self) -> Rc<MemPool> {
		self.mem_pool.clone()
	}
}
