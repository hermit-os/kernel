// Copyright (c) 2020 Frederik Schulz, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! This module contains Virtio's split virtqueue.
//! See Virito specification v1.1. - 2.6
#![allow(dead_code)]
#![allow(unused)]

use super::super::features::Features;
use super::super::transport::pci::{ComCfg, IsrStatus, NotifCfg, NotifCtrl};
use super::error::VirtqError;
use super::{
	AsSliceU8, BuffSpec, Buffer, BufferToken, Bytes, DescrFlags, MemDescr, MemDescrId, MemPool,
	Pinned, Transfer, TransferState, TransferToken, Virtq, VqIndex, VqSize,
};
use crate::arch::mm::paging::{BasePageSize, PageSize};
use crate::arch::mm::{paging, virtualmem, PhysAddr, VirtAddr};
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::convert::TryFrom;
use core::ops::Deref;
use core::sync::atomic::{fence, Ordering};

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
	index: &'static mut u16,
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
	ref_ring: Box<[*mut TransferToken]>,
	avail_ring: AvailRing,
	used_ring: UsedRing,
}

impl DescrRing {
	fn push(&mut self, tkn: TransferToken) -> (Pinned<TransferToken>, u16, u16) {
		let pin = Pinned::pin(tkn);

		let mut desc_lst = Vec::new();
		let mut is_indirect = false;

		if let Some(buff) = pin.buff_tkn.as_ref().unwrap().send_buff.as_ref() {
			if buff.is_indirect() {
				desc_lst.push((buff.get_ctrl_desc().unwrap(), false));
				is_indirect = true;
			} else {
				for desc in buff.as_slice() {
					desc_lst.push((desc, false));
				}
			}
		}

		if let Some(buff) = pin.buff_tkn.as_ref().unwrap().recv_buff.as_ref() {
			if buff.is_indirect() {
				if desc_lst.len() == 0 {
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

		let mut len = pin.buff_tkn.as_ref().unwrap().num_consuming_descr();

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
			} else {
				if len > 1 {
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
				} else {
					if is_write {
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
					}
				}
			};

			self.descr_table.raw[write_indx] = descriptor;

			desc_cnt += 1;
			len -= 1;
		}

		self.ref_ring[index] = pin.raw_addr();
		self.avail_ring.ring[*self.avail_ring.index as usize % self.avail_ring.ring.len()] =
			index as u16;

		fence(Ordering::SeqCst);
		*self.avail_ring.index = self.avail_ring.index.wrapping_add(1);

		(pin, 0, 0)
	}

	fn poll(&mut self) {
		while self.read_idx != *self.used_ring.index {
			let cur_ring_index = self.read_idx as usize % self.used_ring.ring.len();
			let used_elem = self.used_ring.ring[cur_ring_index];

			let tkn = unsafe { &mut *(self.ref_ring[used_elem.id as usize]) };

			if tkn.buff_tkn.as_ref().unwrap().recv_buff.as_ref().is_some() {
				tkn.buff_tkn
					.as_mut()
					.unwrap()
					.restr_size(None, Some(used_elem.len as usize));
			}
			match tkn.await_queue {
				Some(_) => {
					tkn.state = TransferState::Finished;
					let queue = tkn.await_queue.take().unwrap();

					// Turn the raw pointer into a Pinned again, which will hold ownership of the Token
					queue.borrow_mut().push_back(Transfer {
						transfer_tkn: Some(Pinned::from_raw(tkn as *mut TransferToken)),
					});
				}
				None => tkn.state = TransferState::Finished,
			}
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
	dropped: RefCell<Vec<Pinned<TransferToken>>>,
	index: VqIndex,

	notif_ctrl: NotifCtrl,
}

impl SplitVq {
	/// Enables interrupts for this virtqueue upon receiving a transfer
	pub fn enable_notifs(&self) {
		self.ring.borrow_mut().drv_enable_notif();
	}

	/// Disables interrupts for this virtqueue upon receiving a transfer
	pub fn disable_notifs(&self) {
		self.ring.borrow_mut().drv_disable_notif();
	}

	/// This function does check if early dropped TransferTokens are finished
	/// and removes them if this is the case.
	pub fn clean_up(&self) {
		// remove and drop all finished Transfers
		if !self.dropped.borrow().is_empty() {
			self.dropped
				.borrow_mut()
				.drain_filter(|tkn| tkn.state == TransferState::Finished);
		}
	}

	/// See `Virtq.poll()` documentation
	pub fn poll(&self) {
		self.ring.borrow_mut().poll()
	}

	/// Dispatches a batch of transfer token. The buffers of the respective transfers are provided to the queue in
	/// sequence. After the last buffer has been writen, the queue marks the first buffer as available and triggers
	/// a device notification if wanted by the device.
	///
	/// The `notif` parameter indicates if the driver wants to have a notification for this specific
	/// transfer. This is only for performance optimization. As it is NOT ensured, that the device sees the
	/// updated notification flags before finishing transfers!
	pub fn dispatch_batch(&self, tkns: Vec<TransferToken>, notif: bool) -> Vec<Transfer> {
		unimplemented!();
	}

	/// Dispatches a batch of TransferTokens. The Transfers will be placed in to the `await_queue`
	/// upon finish.
	///
	/// The `notif` parameter indicates if the driver wants to have a notification for this specific
	/// transfer. This is only for performance optimization. As it is NOT ensured, that the device sees the
	/// updated notification flags before finishing transfers!
	///
	/// Dispatches a batch of transfer token. The buffers of the respective transfers are provided to the queue in
	/// sequence. After the last buffer has been writen, the queue marks the first buffer as available and triggers
	/// a device notification if wanted by the device.
	///
	/// Tokens to get a reference to the provided await_queue, where they will be placed upon finish.
	pub fn dispatch_batch_await(
		&self,
		tkns: Vec<TransferToken>,
		await_queue: Rc<RefCell<VecDeque<Transfer>>>,
		notif: bool,
	) {
		unimplemented!()
	}

	/// See `Virtq.prep_transfer()` documentation.
	///
	/// The `notif` parameter indicates if the driver wants to have a notification for this specific
	/// transfer. This is only for performance optimization. As it is NOT ensured, that the device sees the
	/// updated notification flags before finishing transfers!
	pub fn dispatch(&self, tkn: TransferToken, notif: bool) -> Transfer {
		let (pin_tkn, next_off, next_wrap) = self.ring.borrow_mut().push(tkn);

		if notif {
			// TODO: Check wheter the splitvirtquue has notifications for specific descriptors
			// I believe it does not.
			unimplemented!();
		}

		if self.ring.borrow().dev_is_notif() {
			let index = self.index.0.to_le_bytes();
			let mut index = index.iter();
			// Even on 64bit systems this is fine, as we have a queue_size < 2^15!
			let det_notif_data: u16 = (next_off as u16) >> 1;
			let flags = (det_notif_data | (u16::from(next_wrap) << 15)).to_le_bytes();
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

		Transfer {
			transfer_tkn: Some(pin_tkn),
		}
	}

	/// The packed virtqueue handles early dropped transfers by moving the respective tokens into
	/// an vector. Here they will remain until they are finished. In order to ensure this the queue
	/// will check theses descriptors from time to time during its poll function.
	///
	/// Also see `Virtq.early_drop()` documentation
	pub fn early_drop(&self, tkn: Pinned<TransferToken>) {
		match tkn.state {
			TransferState::Finished => (), // Drop the pinned token -> Dealloc everything
			TransferState::Ready => {
				unreachable!("Early dropped transfers are not allowed to be state == Ready")
			}
			TransferState::Processing => {
				// Keep token until state is finished. This needs to be checked/cleaned up later
				self.dropped.borrow_mut().push(tkn);
			}
		}
	}

	/// See `Virtq.index()` documentation
	pub fn index(&self) -> VqIndex {
		self.index
	}

	/// See `Virtq::new()` documentation
	pub fn new(
		com_cfg: &mut ComCfg,
		notif_cfg: &NotifCfg,
		size: VqSize,
		index: VqIndex,
		feats: u64,
	) -> Result<Self, ()> {
		// Get a handler to the queues configuration area.
		let mut vq_handler = match com_cfg.select_vq(index.into()) {
			Some(handler) => handler,
			None => return Err(()),
		};

		let size = vq_handler.set_vq_size(size.0);

		// Allocate heap memory via a vec, leak and cast
		let _mem_len = align_up!(
			size as usize * core::mem::size_of::<Descriptor>(),
			BasePageSize::SIZE
		);
		let table_raw =
			(crate::mm::allocate(_mem_len, true).0 as *const Descriptor) as *mut Descriptor;

		let descr_table = DescrTable {
			raw: unsafe { core::slice::from_raw_parts_mut(table_raw, size as usize) },
		};

		let _mem_len = align_up!(6 + (size as usize * 2), BasePageSize::SIZE);
		let avail_raw = (crate::mm::allocate(_mem_len, true).0 as *const u8) as *mut u8;
		let _mem_len = align_up!(6 + (size as usize * 8), BasePageSize::SIZE);
		let used_raw = (crate::mm::allocate(_mem_len, true).0 as *const u8) as *mut u8;

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
			let index = avail_raw.offset(2) as usize - avail_raw as usize;
			let ring = avail_raw.offset(4) as usize - avail_raw as usize;
			let event = avail_raw.offset(4 + 2 * (size as isize)) as usize - avail_raw as usize;
		}

		let used_ring = unsafe {
			UsedRing {
				flags: &mut *(used_raw as *mut u16),
				index: &mut *(used_raw.offset(2) as *mut u16),
				ring: core::slice::from_raw_parts_mut(
					(used_raw.offset(4) as *const _) as *mut UsedElem,
					size as usize,
				),
				event: &mut *(used_raw.offset(4 + 8 * (size as isize)) as *mut u16),
			}
		};

		unsafe {
			let index = used_raw.offset(2) as usize - used_raw as usize;
			let ring = used_raw.offset(4) as usize - used_raw as usize;
			let event = used_raw.offset(4 + 8 * (size as isize)) as usize - used_raw as usize;
		}

		// Provide memory areas of the queues data structures to the device
		vq_handler.set_ring_addr(paging::virt_to_phys(VirtAddr::from(table_raw as u64)));
		// As usize is safe here, as the *mut EventSuppr raw pointer is a thin pointer of size usize
		vq_handler.set_drv_ctrl_addr(paging::virt_to_phys(VirtAddr::from(avail_raw as u64)));
		vq_handler.set_dev_ctrl_addr(paging::virt_to_phys(VirtAddr::from(used_raw as u64)));

		let descr_ring = DescrRing {
			read_idx: 0,
			ref_ring: vec![0 as *mut TransferToken; size as usize].into_boxed_slice(),
			descr_table,
			avail_ring,
			used_ring,
		};

		let notif_ctrl = NotifCtrl::new(
			(notif_cfg.base()
				+ usize::try_from(vq_handler.notif_off()).unwrap()
				+ usize::try_from(notif_cfg.multiplier()).unwrap()) as *mut usize,
		);

		// Initialize new memory pool.
		let mem_pool = Rc::new(MemPool::new(size));

		// Initialize an empty vector for future dropped transfers
		let dropped: RefCell<Vec<Pinned<TransferToken>>> = RefCell::new(Vec::new());

		vq_handler.enable_queue();

		info!("Created SplitVq: idx={}, size={}", index.0, size);

		Ok(SplitVq {
			ring: RefCell::new(descr_ring),
			notif_ctrl,
			mem_pool,
			size: VqSize(size),
			index,
			dropped,
		})
	}

	/// See `Virtq.prep_transfer_from_raw()` documentation.
	pub fn prep_transfer_from_raw<T: AsSliceU8 + 'static, K: AsSliceU8 + 'static>(
		&self,
		master: Rc<Virtq>,
		send: Option<(*mut T, BuffSpec)>,
		recv: Option<(*mut K, BuffSpec)>,
	) -> Result<TransferToken, VirtqError> {
		match (send, recv) {
			(None, None) => return Err(VirtqError::BufferNotSpecified),
			(Some((send_data, send_spec)), None) => {
				match send_spec {
					BuffSpec::Single(size) => {
						let data_slice = unsafe { (*send_data).as_slice_u8() };

						// Buffer must have the right size
						if data_slice.len() != size.into() {
							return Err(VirtqError::BufferSizeWrong(data_slice.len()));
						}

						let desc = match self
							.mem_pool
							.pull_from_raw(Rc::clone(&self.mem_pool), data_slice)
						{
							Ok(desc) => desc,
							Err(vq_err) => return Err(vq_err),
						};

						Ok(TransferToken {
							state: TransferState::Ready,
							buff_tkn: Some(BufferToken {
								send_buff: Some(Buffer::Single {
									desc_lst: vec![desc].into_boxed_slice(),
									len: data_slice.len(),
									next_write: 0,
								}),
								recv_buff: None,
								vq: master,
								ret_send: false,
								ret_recv: false,
								reusable: false,
							}),
							await_queue: None,
						})
					}
					BuffSpec::Multiple(size_lst) => {
						let data_slice = unsafe { (*send_data).as_slice_u8() };
						let mut desc_lst: Vec<MemDescr> = Vec::with_capacity(size_lst.len());
						let mut index = 0usize;

						for byte in size_lst {
							let end_index = index + usize::from(*byte);
							let next_slice = match data_slice.get(index..end_index) {
								Some(slice) => slice,
								None => return Err(VirtqError::BufferSizeWrong(data_slice.len())),
							};

							match self
								.mem_pool
								.pull_from_raw(Rc::clone(&self.mem_pool), next_slice)
							{
								Ok(desc) => desc_lst.push(desc),
								Err(vq_err) => return Err(vq_err),
							};

							// update the starting index for the next iteration
							index = index + usize::from(*byte);
						}

						Ok(TransferToken {
							state: TransferState::Ready,
							buff_tkn: Some(BufferToken {
								send_buff: Some(Buffer::Multiple {
									desc_lst: desc_lst.into_boxed_slice(),
									len: data_slice.len(),
									next_write: 0,
								}),
								recv_buff: None,
								vq: master,
								ret_send: false,
								ret_recv: false,
								reusable: false,
							}),
							await_queue: None,
						})
					}
					BuffSpec::Indirect(size_lst) => {
						let data_slice = unsafe { (*send_data).as_slice_u8() };
						let mut desc_lst: Vec<MemDescr> = Vec::with_capacity(size_lst.len());
						let mut index = 0usize;

						for byte in size_lst {
							let end_index = index + usize::from(*byte);
							let next_slice = match data_slice.get(index..end_index) {
								Some(slice) => slice,
								None => return Err(VirtqError::BufferSizeWrong(data_slice.len())),
							};

							desc_lst.push(
								self.mem_pool
									.pull_from_raw_untracked(Rc::clone(&self.mem_pool), next_slice),
							);

							// update the starting index for the next iteration
							index = index + usize::from(*byte);
						}

						let ctrl_desc = match self.create_indirect_ctrl(Some(&desc_lst), None) {
							Ok(desc) => desc,
							Err(vq_err) => return Err(vq_err),
						};

						Ok(TransferToken {
							state: TransferState::Ready,
							buff_tkn: Some(BufferToken {
								send_buff: Some(Buffer::Indirect {
									desc_lst: desc_lst.into_boxed_slice(),
									ctrl_desc: ctrl_desc,
									len: data_slice.len(),
									next_write: 0,
								}),
								recv_buff: None,
								vq: master,
								ret_send: false,
								ret_recv: false,
								reusable: false,
							}),
							await_queue: None,
						})
					}
				}
			}
			(None, Some((recv_data, recv_spec))) => {
				match recv_spec {
					BuffSpec::Single(size) => {
						let data_slice = unsafe { (*recv_data).as_slice_u8() };

						// Buffer must have the right size
						if data_slice.len() != size.into() {
							return Err(VirtqError::BufferSizeWrong(data_slice.len()));
						}

						let desc = match self
							.mem_pool
							.pull_from_raw(Rc::clone(&self.mem_pool), data_slice)
						{
							Ok(desc) => desc,
							Err(vq_err) => return Err(vq_err),
						};

						Ok(TransferToken {
							state: TransferState::Ready,
							buff_tkn: Some(BufferToken {
								send_buff: None,
								recv_buff: Some(Buffer::Single {
									desc_lst: vec![desc].into_boxed_slice(),
									len: data_slice.len(),
									next_write: 0,
								}),
								vq: master,
								ret_send: false,
								ret_recv: false,
								reusable: false,
							}),
							await_queue: None,
						})
					}
					BuffSpec::Multiple(size_lst) => {
						let data_slice = unsafe { (*recv_data).as_slice_u8() };
						let mut desc_lst: Vec<MemDescr> = Vec::with_capacity(size_lst.len());
						let mut index = 0usize;

						for byte in size_lst {
							let end_index = index + usize::from(*byte);
							let next_slice = match data_slice.get(index..end_index) {
								Some(slice) => slice,
								None => return Err(VirtqError::BufferSizeWrong(data_slice.len())),
							};

							match self
								.mem_pool
								.pull_from_raw(Rc::clone(&self.mem_pool), next_slice)
							{
								Ok(desc) => desc_lst.push(desc),
								Err(vq_err) => return Err(vq_err),
							};

							// update the starting index for the next iteration
							index = index + usize::from(*byte);
						}

						Ok(TransferToken {
							state: TransferState::Ready,
							buff_tkn: Some(BufferToken {
								send_buff: None,
								recv_buff: Some(Buffer::Multiple {
									desc_lst: desc_lst.into_boxed_slice(),
									len: data_slice.len(),
									next_write: 0,
								}),
								vq: master,
								ret_send: false,
								ret_recv: false,
								reusable: false,
							}),
							await_queue: None,
						})
					}
					BuffSpec::Indirect(size_lst) => {
						let data_slice = unsafe { (*recv_data).as_slice_u8() };
						let mut desc_lst: Vec<MemDescr> = Vec::with_capacity(size_lst.len());
						let mut index = 0usize;

						for byte in size_lst {
							let end_index = index + usize::from(*byte);
							let next_slice = match data_slice.get(index..end_index) {
								Some(slice) => slice,
								None => return Err(VirtqError::BufferSizeWrong(data_slice.len())),
							};

							desc_lst.push(
								self.mem_pool
									.pull_from_raw_untracked(Rc::clone(&self.mem_pool), next_slice),
							);

							// update the starting index for the next iteration
							index = index + usize::from(*byte);
						}

						let ctrl_desc = match self.create_indirect_ctrl(None, Some(&desc_lst)) {
							Ok(desc) => desc,
							Err(vq_err) => return Err(vq_err),
						};

						Ok(TransferToken {
							state: TransferState::Ready,
							buff_tkn: Some(BufferToken {
								send_buff: None,
								recv_buff: Some(Buffer::Indirect {
									desc_lst: desc_lst.into_boxed_slice(),
									ctrl_desc: ctrl_desc,
									len: data_slice.len(),
									next_write: 0,
								}),
								vq: master,
								ret_send: false,
								ret_recv: false,
								reusable: false,
							}),
							await_queue: None,
						})
					}
				}
			}
			(Some((send_data, send_spec)), Some((recv_data, recv_spec))) => {
				match (send_spec, recv_spec) {
					(BuffSpec::Single(send_size), BuffSpec::Single(recv_size)) => {
						let send_data_slice = unsafe { (*send_data).as_slice_u8() };

						// Buffer must have the right size
						if send_data_slice.len() != send_size.into() {
							return Err(VirtqError::BufferSizeWrong(send_data_slice.len()));
						}

						let send_desc = match self
							.mem_pool
							.pull_from_raw(Rc::clone(&self.mem_pool), send_data_slice)
						{
							Ok(desc) => desc,
							Err(vq_err) => return Err(vq_err),
						};

						let recv_data_slice = unsafe { (*recv_data).as_slice_u8() };

						// Buffer must have the right size
						if recv_data_slice.len() != recv_size.into() {
							return Err(VirtqError::BufferSizeWrong(recv_data_slice.len()));
						}

						let recv_desc = match self
							.mem_pool
							.pull_from_raw(Rc::clone(&self.mem_pool), recv_data_slice)
						{
							Ok(desc) => desc,
							Err(vq_err) => return Err(vq_err),
						};

						Ok(TransferToken {
							state: TransferState::Ready,
							buff_tkn: Some(BufferToken {
								send_buff: Some(Buffer::Single {
									desc_lst: vec![send_desc].into_boxed_slice(),
									len: send_data_slice.len(),
									next_write: 0,
								}),
								recv_buff: Some(Buffer::Single {
									desc_lst: vec![recv_desc].into_boxed_slice(),
									len: recv_data_slice.len(),
									next_write: 0,
								}),
								vq: master,
								ret_send: false,
								ret_recv: false,
								reusable: false,
							}),
							await_queue: None,
						})
					}
					(BuffSpec::Single(send_size), BuffSpec::Multiple(recv_size_lst)) => {
						let send_data_slice = unsafe { (*send_data).as_slice_u8() };

						// Buffer must have the right size
						if send_data_slice.len() != send_size.into() {
							return Err(VirtqError::BufferSizeWrong(send_data_slice.len()));
						}

						let send_desc = match self
							.mem_pool
							.pull_from_raw(Rc::clone(&self.mem_pool), send_data_slice)
						{
							Ok(desc) => desc,
							Err(vq_err) => return Err(vq_err),
						};

						let recv_data_slice = unsafe { (*recv_data).as_slice_u8() };
						let mut recv_desc_lst: Vec<MemDescr> =
							Vec::with_capacity(recv_size_lst.len());
						let mut index = 0usize;

						for byte in recv_size_lst {
							let end_index = index + usize::from(*byte);
							let next_slice = match recv_data_slice.get(index..end_index) {
								Some(slice) => slice,
								None => {
									return Err(VirtqError::BufferSizeWrong(recv_data_slice.len()))
								}
							};

							match self
								.mem_pool
								.pull_from_raw(Rc::clone(&self.mem_pool), next_slice)
							{
								Ok(desc) => recv_desc_lst.push(desc),
								Err(vq_err) => return Err(vq_err),
							};

							// update the starting index for the next iteration
							index = index + usize::from(*byte);
						}

						Ok(TransferToken {
							state: TransferState::Ready,
							buff_tkn: Some(BufferToken {
								send_buff: Some(Buffer::Single {
									desc_lst: vec![send_desc].into_boxed_slice(),
									len: send_data_slice.len(),
									next_write: 0,
								}),
								recv_buff: Some(Buffer::Multiple {
									desc_lst: recv_desc_lst.into_boxed_slice(),
									len: recv_data_slice.len(),
									next_write: 0,
								}),
								vq: master,
								ret_send: false,
								ret_recv: false,
								reusable: false,
							}),
							await_queue: None,
						})
					}
					(BuffSpec::Multiple(send_size_lst), BuffSpec::Multiple(recv_size_lst)) => {
						let send_data_slice = unsafe { (*send_data).as_slice_u8() };
						let mut send_desc_lst: Vec<MemDescr> =
							Vec::with_capacity(send_size_lst.len());
						let mut index = 0usize;

						for byte in send_size_lst {
							let end_index = index + usize::from(*byte);
							let next_slice = match send_data_slice.get(index..end_index) {
								Some(slice) => slice,
								None => {
									return Err(VirtqError::BufferSizeWrong(send_data_slice.len()))
								}
							};

							match self
								.mem_pool
								.pull_from_raw(Rc::clone(&self.mem_pool), next_slice)
							{
								Ok(desc) => send_desc_lst.push(desc),
								Err(vq_err) => return Err(vq_err),
							};

							// update the starting index for the next iteration
							index = index + usize::from(*byte);
						}

						let recv_data_slice = unsafe { (*recv_data).as_slice_u8() };
						let mut recv_desc_lst: Vec<MemDescr> =
							Vec::with_capacity(recv_size_lst.len());
						let mut index = 0usize;

						for byte in recv_size_lst {
							let end_index = index + usize::from(*byte);
							let next_slice = match recv_data_slice.get(index..end_index) {
								Some(slice) => slice,
								None => {
									return Err(VirtqError::BufferSizeWrong(recv_data_slice.len()))
								}
							};

							match self
								.mem_pool
								.pull_from_raw(Rc::clone(&self.mem_pool), next_slice)
							{
								Ok(desc) => recv_desc_lst.push(desc),
								Err(vq_err) => return Err(vq_err),
							};

							// update the starting index for the next iteration
							index = index + usize::from(*byte);
						}

						Ok(TransferToken {
							state: TransferState::Ready,
							buff_tkn: Some(BufferToken {
								send_buff: Some(Buffer::Multiple {
									desc_lst: send_desc_lst.into_boxed_slice(),
									len: send_data_slice.len(),
									next_write: 0,
								}),
								recv_buff: Some(Buffer::Multiple {
									desc_lst: recv_desc_lst.into_boxed_slice(),
									len: recv_data_slice.len(),
									next_write: 0,
								}),
								vq: master,
								ret_send: false,
								ret_recv: false,
								reusable: false,
							}),
							await_queue: None,
						})
					}
					(BuffSpec::Multiple(send_size_lst), BuffSpec::Single(recv_size)) => {
						let send_data_slice = unsafe { (*send_data).as_slice_u8() };
						let mut send_desc_lst: Vec<MemDescr> =
							Vec::with_capacity(send_size_lst.len());
						let mut index = 0usize;

						for byte in send_size_lst {
							let end_index = index + usize::from(*byte);
							let next_slice = match send_data_slice.get(index..end_index) {
								Some(slice) => slice,
								None => {
									return Err(VirtqError::BufferSizeWrong(send_data_slice.len()))
								}
							};

							match self
								.mem_pool
								.pull_from_raw(Rc::clone(&self.mem_pool), next_slice)
							{
								Ok(desc) => send_desc_lst.push(desc),
								Err(vq_err) => return Err(vq_err),
							};

							// update the starting index for the next iteration
							index = index + usize::from(*byte);
						}

						let recv_data_slice = unsafe { (*recv_data).as_slice_u8() };

						// Buffer must have the right size
						if recv_data_slice.len() != recv_size.into() {
							return Err(VirtqError::BufferSizeWrong(recv_data_slice.len()));
						}

						let recv_desc = match self
							.mem_pool
							.pull_from_raw(Rc::clone(&self.mem_pool), recv_data_slice)
						{
							Ok(desc) => desc,
							Err(vq_err) => return Err(vq_err),
						};

						Ok(TransferToken {
							state: TransferState::Ready,
							buff_tkn: Some(BufferToken {
								send_buff: Some(Buffer::Multiple {
									desc_lst: send_desc_lst.into_boxed_slice(),
									len: send_data_slice.len(),
									next_write: 0,
								}),
								recv_buff: Some(Buffer::Single {
									desc_lst: vec![recv_desc].into_boxed_slice(),
									len: recv_data_slice.len(),
									next_write: 0,
								}),
								vq: master,
								ret_send: false,
								ret_recv: false,
								reusable: false,
							}),
							await_queue: None,
						})
					}
					(BuffSpec::Indirect(send_size_lst), BuffSpec::Indirect(recv_size_lst)) => {
						let send_data_slice = unsafe { (*send_data).as_slice_u8() };
						let mut send_desc_lst: Vec<MemDescr> =
							Vec::with_capacity(send_size_lst.len());
						let mut index = 0usize;

						for byte in send_size_lst {
							let end_index = index + usize::from(*byte);
							let next_slice = match send_data_slice.get(index..end_index) {
								Some(slice) => slice,
								None => {
									return Err(VirtqError::BufferSizeWrong(send_data_slice.len()))
								}
							};

							send_desc_lst.push(
								self.mem_pool
									.pull_from_raw_untracked(Rc::clone(&self.mem_pool), next_slice),
							);

							// update the starting index for the next iteration
							index = index + usize::from(*byte);
						}

						let recv_data_slice = unsafe { (*recv_data).as_slice_u8() };
						let mut recv_desc_lst: Vec<MemDescr> =
							Vec::with_capacity(recv_size_lst.len());
						let mut index = 0usize;

						for byte in recv_size_lst {
							let end_index = index + usize::from(*byte);
							let next_slice = match recv_data_slice.get(index..end_index) {
								Some(slice) => slice,
								None => {
									return Err(VirtqError::BufferSizeWrong(recv_data_slice.len()))
								}
							};

							recv_desc_lst.push(
								self.mem_pool
									.pull_from_raw_untracked(Rc::clone(&self.mem_pool), next_slice),
							);

							// update the starting index for the next iteration
							index = index + usize::from(*byte);
						}

						let ctrl_desc = match self
							.create_indirect_ctrl(Some(&send_desc_lst), Some(&recv_desc_lst))
						{
							Ok(desc) => desc,
							Err(vq_err) => return Err(vq_err),
						};

						Ok(TransferToken {
							state: TransferState::Ready,
							buff_tkn: Some(BufferToken {
								recv_buff: Some(Buffer::Indirect {
									desc_lst: recv_desc_lst.into_boxed_slice(),
									ctrl_desc: ctrl_desc.no_dealloc_clone(),
									len: recv_data_slice.len(),
									next_write: 0,
								}),
								send_buff: Some(Buffer::Indirect {
									desc_lst: send_desc_lst.into_boxed_slice(),
									ctrl_desc: ctrl_desc,
									len: send_data_slice.len(),
									next_write: 0,
								}),
								vq: master,
								ret_send: false,
								ret_recv: false,
								reusable: false,
							}),
							await_queue: None,
						})
					}
					(BuffSpec::Indirect(_), BuffSpec::Single(_))
					| (BuffSpec::Indirect(_), BuffSpec::Multiple(_)) => {
						return Err(VirtqError::BufferInWithDirect)
					}
					(BuffSpec::Single(_), BuffSpec::Indirect(_))
					| (BuffSpec::Multiple(_), BuffSpec::Indirect(_)) => {
						return Err(VirtqError::BufferInWithDirect)
					}
				}
			}
		}
	}

	/// See `Virtq.prep_buffer()` documentation.
	pub fn prep_buffer(
		&self,
		master: Rc<Virtq>,
		send: Option<BuffSpec>,
		recv: Option<BuffSpec>,
	) -> Result<BufferToken, VirtqError> {
		match (send, recv) {
			// No buffers specified
			(None, None) => return Err(VirtqError::BufferNotSpecified),
			// Send buffer specified, No recv buffer
			(Some(spec), None) => {
				match spec {
					BuffSpec::Single(size) => {
						match self.mem_pool.pull(Rc::clone(&self.mem_pool), size) {
							Ok(desc) => {
								let buffer = Buffer::Single {
									desc_lst: vec![desc].into_boxed_slice(),
									len: size.into(),
									next_write: 0,
								};

								Ok(BufferToken {
									send_buff: Some(buffer),
									recv_buff: None,
									vq: master,
									ret_send: true,
									ret_recv: false,
									reusable: true,
								})
							}
							Err(vq_err) => return Err(vq_err),
						}
					}
					BuffSpec::Multiple(size_lst) => {
						let mut desc_lst: Vec<MemDescr> = Vec::with_capacity(size_lst.len());
						let mut len = 0usize;

						for size in size_lst {
							match self.mem_pool.pull(Rc::clone(&self.mem_pool), *size) {
								Ok(desc) => desc_lst.push(desc),
								Err(vq_err) => return Err(vq_err),
							}
							len += usize::from(*size);
						}

						let buffer = Buffer::Multiple {
							desc_lst: desc_lst.into_boxed_slice(),
							len,
							next_write: 0,
						};

						Ok(BufferToken {
							send_buff: Some(buffer),
							recv_buff: None,
							vq: master,
							ret_send: true,
							ret_recv: false,
							reusable: true,
						})
					}
					BuffSpec::Indirect(size_lst) => {
						let mut desc_lst: Vec<MemDescr> = Vec::with_capacity(size_lst.len());
						let mut len = 0usize;

						for size in size_lst {
							// As the indirect list does only consume one descriptor for the
							// control descriptor, the actual list is untracked
							desc_lst.push(
								self.mem_pool
									.pull_untracked(Rc::clone(&self.mem_pool), *size),
							);
							len += usize::from(*size);
						}

						let ctrl_desc = match self.create_indirect_ctrl(Some(&desc_lst), None) {
							Ok(desc) => desc,
							Err(vq_err) => return Err(vq_err),
						};

						let buffer = Buffer::Indirect {
							desc_lst: desc_lst.into_boxed_slice(),
							ctrl_desc,
							len,
							next_write: 0,
						};

						Ok(BufferToken {
							send_buff: Some(buffer),
							recv_buff: None,
							vq: master,
							ret_send: true,
							ret_recv: false,
							reusable: true,
						})
					}
				}
			}
			// No send buffer, recv buffer is specified
			(None, Some(spec)) => {
				match spec {
					BuffSpec::Single(size) => {
						match self.mem_pool.pull(Rc::clone(&self.mem_pool), size) {
							Ok(desc) => {
								let buffer = Buffer::Single {
									desc_lst: vec![desc].into_boxed_slice(),
									len: size.into(),
									next_write: 0,
								};

								Ok(BufferToken {
									send_buff: None,
									recv_buff: Some(buffer),
									vq: master,
									ret_send: false,
									ret_recv: true,
									reusable: true,
								})
							}
							Err(vq_err) => return Err(vq_err),
						}
					}
					BuffSpec::Multiple(size_lst) => {
						let mut desc_lst: Vec<MemDescr> = Vec::with_capacity(size_lst.len());
						let mut len = 0usize;

						for size in size_lst {
							match self.mem_pool.pull(Rc::clone(&self.mem_pool), *size) {
								Ok(desc) => desc_lst.push(desc),
								Err(vq_err) => return Err(vq_err),
							}
							len += usize::from(*size);
						}

						let buffer = Buffer::Multiple {
							desc_lst: desc_lst.into_boxed_slice(),
							len,
							next_write: 0,
						};

						Ok(BufferToken {
							send_buff: None,
							recv_buff: Some(buffer),
							vq: master,
							ret_send: false,
							ret_recv: true,
							reusable: true,
						})
					}
					BuffSpec::Indirect(size_lst) => {
						let mut desc_lst: Vec<MemDescr> = Vec::with_capacity(size_lst.len());
						let mut len = 0usize;

						for size in size_lst {
							// As the indirect list does only consume one descriptor for the
							// control descriptor, the actual list is untracked
							desc_lst.push(
								self.mem_pool
									.pull_untracked(Rc::clone(&self.mem_pool), *size),
							);
							len += usize::from(*size);
						}

						let ctrl_desc = match self.create_indirect_ctrl(None, Some(&desc_lst)) {
							Ok(desc) => desc,
							Err(vq_err) => return Err(vq_err),
						};

						let buffer = Buffer::Indirect {
							desc_lst: desc_lst.into_boxed_slice(),
							ctrl_desc,
							len,
							next_write: 0,
						};

						Ok(BufferToken {
							send_buff: None,
							recv_buff: Some(buffer),
							vq: master,
							ret_send: false,
							ret_recv: true,
							reusable: true,
						})
					}
				}
			}
			// Send buffer specified, recv buffer specified
			(Some(send_spec), Some(recv_spec)) => {
				match (send_spec, recv_spec) {
					(BuffSpec::Single(send_size), BuffSpec::Single(recv_size)) => {
						let send_buff =
							match self.mem_pool.pull(Rc::clone(&self.mem_pool), send_size) {
								Ok(send_desc) => Some(Buffer::Single {
									desc_lst: vec![send_desc].into_boxed_slice(),
									len: send_size.into(),
									next_write: 0,
								}),
								Err(vq_err) => return Err(vq_err),
							};

						let recv_buff =
							match self.mem_pool.pull(Rc::clone(&self.mem_pool), recv_size) {
								Ok(recv_desc) => Some(Buffer::Single {
									desc_lst: vec![recv_desc].into_boxed_slice(),
									len: recv_size.into(),
									next_write: 0,
								}),
								Err(vq_err) => return Err(vq_err),
							};

						Ok(BufferToken {
							send_buff,
							recv_buff,
							vq: master,
							ret_send: true,
							ret_recv: true,
							reusable: true,
						})
					}
					(BuffSpec::Single(send_size), BuffSpec::Multiple(recv_size_lst)) => {
						let send_buff =
							match self.mem_pool.pull(Rc::clone(&self.mem_pool), send_size) {
								Ok(send_desc) => Some(Buffer::Single {
									desc_lst: vec![send_desc].into_boxed_slice(),
									len: send_size.into(),
									next_write: 0,
								}),
								Err(vq_err) => return Err(vq_err),
							};

						let mut recv_desc_lst: Vec<MemDescr> =
							Vec::with_capacity(recv_size_lst.len());
						let mut recv_len = 0usize;

						for size in recv_size_lst {
							match self.mem_pool.pull(Rc::clone(&self.mem_pool), *size) {
								Ok(desc) => recv_desc_lst.push(desc),
								Err(vq_err) => return Err(vq_err),
							}
							recv_len += usize::from(*size);
						}

						let recv_buff = Some(Buffer::Multiple {
							desc_lst: recv_desc_lst.into_boxed_slice(),
							len: recv_len,
							next_write: 0,
						});

						Ok(BufferToken {
							send_buff,
							recv_buff,
							vq: master,
							ret_send: true,
							ret_recv: true,
							reusable: true,
						})
					}
					(BuffSpec::Multiple(send_size_lst), BuffSpec::Multiple(recv_size_lst)) => {
						let mut send_desc_lst: Vec<MemDescr> =
							Vec::with_capacity(send_size_lst.len());
						let mut send_len = 0usize;
						for size in send_size_lst {
							match self.mem_pool.pull(Rc::clone(&self.mem_pool), *size) {
								Ok(desc) => send_desc_lst.push(desc),
								Err(vq_err) => return Err(vq_err),
							}
							send_len += usize::from(*size);
						}

						let send_buff = Some(Buffer::Multiple {
							desc_lst: send_desc_lst.into_boxed_slice(),
							len: send_len,
							next_write: 0,
						});

						let mut recv_desc_lst: Vec<MemDescr> =
							Vec::with_capacity(recv_size_lst.len());
						let mut recv_len = 0usize;

						for size in recv_size_lst {
							match self.mem_pool.pull(Rc::clone(&self.mem_pool), *size) {
								Ok(desc) => recv_desc_lst.push(desc),
								Err(vq_err) => return Err(vq_err),
							}
							recv_len += usize::from(*size);
						}

						let recv_buff = Some(Buffer::Multiple {
							desc_lst: recv_desc_lst.into_boxed_slice(),
							len: recv_len,
							next_write: 0,
						});

						Ok(BufferToken {
							send_buff,
							recv_buff,
							vq: master,
							ret_send: true,
							ret_recv: true,
							reusable: true,
						})
					}
					(BuffSpec::Multiple(send_size_lst), BuffSpec::Single(recv_size)) => {
						let mut send_desc_lst: Vec<MemDescr> =
							Vec::with_capacity(send_size_lst.len());
						let mut send_len = 0usize;

						for size in send_size_lst {
							match self.mem_pool.pull(Rc::clone(&self.mem_pool), *size) {
								Ok(desc) => send_desc_lst.push(desc),
								Err(vq_err) => return Err(vq_err),
							}
							send_len += usize::from(*size);
						}

						let send_buff = Some(Buffer::Multiple {
							desc_lst: send_desc_lst.into_boxed_slice(),
							len: send_len,
							next_write: 0,
						});

						let recv_buff =
							match self.mem_pool.pull(Rc::clone(&self.mem_pool), recv_size) {
								Ok(recv_desc) => Some(Buffer::Single {
									desc_lst: vec![recv_desc].into_boxed_slice(),
									len: recv_size.into(),
									next_write: 0,
								}),
								Err(vq_err) => return Err(vq_err),
							};

						Ok(BufferToken {
							send_buff,
							recv_buff,
							vq: master,
							ret_send: true,
							ret_recv: true,
							reusable: true,
						})
					}
					(BuffSpec::Indirect(send_size_lst), BuffSpec::Indirect(recv_size_lst)) => {
						let mut send_desc_lst: Vec<MemDescr> =
							Vec::with_capacity(send_size_lst.len());
						let mut send_len = 0usize;

						for size in send_size_lst {
							// As the indirect list does only consume one descriptor for the
							// control descriptor, the actual list is untracked
							send_desc_lst.push(
								self.mem_pool
									.pull_untracked(Rc::clone(&self.mem_pool), *size),
							);
							send_len += usize::from(*size);
						}

						let mut recv_desc_lst: Vec<MemDescr> =
							Vec::with_capacity(recv_size_lst.len());
						let mut recv_len = 0usize;

						for size in recv_size_lst {
							// As the indirect list does only consume one descriptor for the
							// control descriptor, the actual list is untracked
							recv_desc_lst.push(
								self.mem_pool
									.pull_untracked(Rc::clone(&self.mem_pool), *size),
							);
							recv_len += usize::from(*size);
						}

						let ctrl_desc = match self
							.create_indirect_ctrl(Some(&send_desc_lst), Some(&recv_desc_lst))
						{
							Ok(desc) => desc,
							Err(vq_err) => return Err(vq_err),
						};

						let recv_buff = Some(Buffer::Indirect {
							desc_lst: recv_desc_lst.into_boxed_slice(),
							ctrl_desc: ctrl_desc.no_dealloc_clone(),
							len: recv_len,
							next_write: 0,
						});
						let send_buff = Some(Buffer::Indirect {
							desc_lst: send_desc_lst.into_boxed_slice(),
							ctrl_desc,
							len: send_len,
							next_write: 0,
						});

						Ok(BufferToken {
							send_buff,
							recv_buff,
							vq: master,
							ret_send: true,
							ret_recv: true,
							reusable: true,
						})
					}
					(BuffSpec::Indirect(_), BuffSpec::Single(_))
					| (BuffSpec::Indirect(_), BuffSpec::Multiple(_)) => {
						return Err(VirtqError::BufferInWithDirect)
					}
					(BuffSpec::Single(_), BuffSpec::Indirect(_))
					| (BuffSpec::Multiple(_), BuffSpec::Indirect(_)) => {
						return Err(VirtqError::BufferInWithDirect)
					}
				}
			}
		}
	}

	pub fn size(&self) -> VqSize {
		self.size
	}
}

// Private Interface for PackedVq
impl SplitVq {
	fn create_indirect_ctrl(
		&self,
		send: Option<&Vec<MemDescr>>,
		recv: Option<&Vec<MemDescr>>,
	) -> Result<MemDescr, VirtqError> {
		// Need to match (send, recv) twice, as the "size" of the control descriptor to be pulled must be known in advance.
		let len: usize;
		match (send, recv) {
			(None, None) => return Err(VirtqError::BufferNotSpecified),
			(None, Some(recv_desc_lst)) => {
				len = recv_desc_lst.len();
			}
			(Some(send_desc_lst), None) => {
				len = send_desc_lst.len();
			}
			(Some(send_desc_lst), Some(recv_desc_lst)) => {
				len = send_desc_lst.len() + recv_desc_lst.len();
			}
		}

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
			(None, None) => return Err(VirtqError::BufferNotSpecified),
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
			// Only sending descritpors
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
}
