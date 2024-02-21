//! This module contains Virtio's split virtqueue.
//! See Virito specification v1.1. - 2.6
#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::ptr;

use align_address::Align;

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
use crate::arch::mm::paging::{BasePageSize, PageSize};
use crate::arch::mm::{paging, VirtAddr};

#[repr(C)]
#[derive(Copy, Clone)]
struct Descriptor {
	address: u64,
	len: u32,
	flags: u16,
	next: u16,
}

impl Descriptor {
	fn new(addr: u64, len: u32, flags: u16, next: u16) -> Self {
		Descriptor {
			address: addr,
			len,
			flags,
			next,
		}
	}
}

struct DescrTable {
	raw: &'static mut [Descriptor],
}

struct AvailRing {
	flags: &'static mut u16,
	index: &'static mut u16,
	ring: &'static mut [u16],
	event: &'static mut u16,
}

struct UsedRing {
	flags: &'static mut u16,
	index: *mut u16,
	ring: &'static mut [UsedElem],
	event: &'static mut u16,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct UsedElem {
	id: u32,
	len: u32,
}

struct DescrRing {
	read_idx: u16,
	descr_table: DescrTable,
	ref_ring: Box<[Option<Box<TransferToken>>]>,
	avail_ring: AvailRing,
	used_ring: UsedRing,
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

			self.descr_table.raw[write_indx] = descriptor;

			desc_cnt += 1;
			len -= 1;
		}

		self.ref_ring[index] = Some(Box::new(tkn));
		self.avail_ring.ring[*self.avail_ring.index as usize % self.avail_ring.ring.len()] =
			index as u16;

		memory_barrier();
		*self.avail_ring.index = self.avail_ring.index.wrapping_add(1);

		(0, 0)
	}

	fn poll(&mut self) {
		while self.read_idx != unsafe { ptr::read_volatile(self.used_ring.index) } {
			let cur_ring_index = self.read_idx as usize % self.used_ring.ring.len();
			let used_elem = unsafe { ptr::read_volatile(&self.used_ring.ring[cur_ring_index]) };

			let mut tkn = self.ref_ring[used_elem.id as usize].take().expect(
				"The buff_id is incorrect or the reference to the TransferToken was misplaced.",
			);

			if tkn.buff_tkn.as_ref().unwrap().recv_buff.as_ref().is_some() {
				tkn.buff_tkn
					.as_mut()
					.unwrap()
					.restr_size(None, Some(used_elem.len as usize))
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
		*self.avail_ring.flags = 0;
	}

	fn drv_disable_notif(&mut self) {
		*self.avail_ring.flags = 1;
	}

	fn dev_is_notif(&self) -> bool {
		*self.used_ring.flags & 1 == 0
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

		// Allocate heap memory via a vec, leak and cast
		let _mem_len = (size as usize * core::mem::size_of::<Descriptor>())
			.align_up(BasePageSize::SIZE as usize);
		let table_raw = ptr::from_exposed_addr_mut(crate::mm::allocate(_mem_len, true).0 as usize);

		let descr_table = DescrTable {
			raw: unsafe { core::slice::from_raw_parts_mut(table_raw, size as usize) },
		};

		let _mem_len = (6 + (size as usize * 2)).align_up(BasePageSize::SIZE as usize);
		let avail_raw =
			ptr::from_exposed_addr_mut::<u8>(crate::mm::allocate(_mem_len, true).0 as usize);
		let _mem_len = (6 + (size as usize * 8)).align_up(BasePageSize::SIZE as usize);
		let used_raw =
			ptr::from_exposed_addr_mut::<u8>(crate::mm::allocate(_mem_len, true).0 as usize);

		let avail_ring = unsafe {
			AvailRing {
				flags: &mut *(avail_raw as *mut u16),
				index: &mut *(avail_raw.offset(2) as *mut u16),
				ring: core::slice::from_raw_parts_mut(
					avail_raw.offset(4) as *mut u16,
					size as usize,
				),
				event: &mut *(avail_raw.offset(4 + 2 * (size as isize)) as *mut u16),
			}
		};

		unsafe {
			let _index = avail_raw.offset(2) as usize - avail_raw as usize;
			let _ring = avail_raw.offset(4) as usize - avail_raw as usize;
			let _event = avail_raw.offset(4 + 2 * (size as isize)) as usize - avail_raw as usize;
		}

		let used_ring = unsafe {
			UsedRing {
				flags: &mut *(used_raw as *mut u16),
				index: used_raw.offset(2) as *mut u16,
				ring: core::slice::from_raw_parts_mut(
					used_raw.offset(4) as *mut UsedElem,
					size as usize,
				),
				event: &mut *(used_raw.offset(4 + 8 * (size as isize)) as *mut u16),
			}
		};

		unsafe {
			let _index = used_raw.offset(2) as usize - used_raw as usize;
			let _ring = used_raw.offset(4) as usize - used_raw as usize;
			let _event = used_raw.offset(4 + 8 * (size as isize)) as usize - used_raw as usize;
		}

		// Provide memory areas of the queues data structures to the device
		vq_handler.set_ring_addr(paging::virt_to_phys(VirtAddr::from(table_raw as u64)));
		// As usize is safe here, as the *mut EventSuppr raw pointer is a thin pointer of size usize
		vq_handler.set_drv_ctrl_addr(paging::virt_to_phys(VirtAddr::from(avail_raw as u64)));
		vq_handler.set_dev_ctrl_addr(paging::virt_to_phys(VirtAddr::from(used_raw as u64)));

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

		let notif_ctrl = NotifCtrl::new(ptr::from_exposed_addr_mut(
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
