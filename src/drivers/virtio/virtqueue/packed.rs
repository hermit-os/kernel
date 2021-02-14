// Copyright (c) 2020 Frederik Schulz, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! This module contains Virtio's packed virtqueue.
//! See Virito specification v1.1. - 2.7
#![allow(dead_code)]
#![allow(unused)]

use self::error::VqPackedError;
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

/// A newtype of bool used for convenience in context with
/// packed queues wrap counter.
///
/// For more details see Virtio specification v1.1. - 2.7.1
#[derive(Copy, Clone, Debug)]
struct WrapCount(bool);

impl WrapCount {
	/// Masks all other bits, besides the wrap count specific ones.
	fn flag_mask() -> u16 {
		1 << 7 | 1 << 15
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
		if self.0 == false {
			self.0 = true;
		} else {
			self.0 = false;
		}
	}

	/// Creates avail and used flags inside u16 in accordance to the
	/// virito specification v1.1. - 2.7.1
	///
	/// I.e.: Set avail flag to match the WrapCount and the used flag
	/// to NOT match the WrapCount.
	fn as_flags_avail(&self) -> u16 {
		if self.0 == true {
			1 << 7
		} else {
			1 << 15
		}
	}

	/// Creates avail and used flags inside u16 in accordance to the
	/// virito specification v1.1. - 2.7.1
	///
	/// I.e.: Set avail flag to match the WrapCount and the used flag
	/// to also match the WrapCount.
	fn as_flags_used(&self) -> u16 {
		if self.0 == true {
			1 << 7 | 1 << 15
		} else {
			0
		}
	}
}

/// Structure which allows to control raw ring and operate easily on it
///
/// WARN: NEVER PUSH TO THE RING AFTER DESCRIPTORRING HAS BEEN INITALIZED AS THIS WILL PROBABLY RESULT IN A
/// RELOCATION OF THE VECTOR AND HENCE THE DEVICE WILL NO LONGER NO THE RINGS ADDRESS!
struct DescriptorRing {
	ring: &'static mut [Descriptor],
	//ring: Pinned<Vec<Descriptor>>,
	tkn_ref_ring: Box<[*mut TransferToken]>,

	// Controlling variables for the ring
	//
	/// where to insert availble descriptors next
	write_index: usize,
	/// How much descriptors can be inserted
	capacity: usize,
	/// Where to expect the next used descriptor by the device
	poll_index: usize,
	/// See Virtio specification v1.1. - 2.7.1
	drv_wc: WrapCount,
	dev_wc: WrapCount,
}

impl DescriptorRing {
	fn new(size: u16) -> Self {
		let size = usize::try_from(size).unwrap();

		// Allocate heap memory via a vec, leak and cast
		let _mem_len = align_up!(
			size * core::mem::size_of::<Descriptor>(),
			BasePageSize::SIZE
		);
		let ptr = (crate::mm::allocate(_mem_len, true).0 as *const Descriptor) as *mut Descriptor;

		let ring: &'static mut [Descriptor] = unsafe { core::slice::from_raw_parts_mut(ptr, size) };

		// Descriptor ID's run from 1 to size_of_queue. In order to index directly into the
		// refernece ring via an ID it is much easier to simply have an array of size = size_of_queue + 1
		// and do not care about the first element beeing unused.
		let tkn_ref_ring = vec![0usize as *mut TransferToken; size + 1].into_boxed_slice();

		DescriptorRing {
			ring,
			tkn_ref_ring,
			write_index: 0,
			capacity: size,
			poll_index: 0,
			drv_wc: WrapCount::new(),
			dev_wc: WrapCount::new(),
		}
	}

	/// Polls poll index and sets states of eventually used TransferTokens to finished.
	/// If Await_qeue is available, the Transfer will be provieded to the queue.
	fn poll(&mut self) {
		let mut ctrl = self.get_read_ctrler();

		if let Some(tkn) = ctrl.poll_next() {
			// The state of the TransferToken up to this point MUST NOT be
			// finished. As soon as we mark the token as finished, we can not
			// be sure, that the token is not dropped, which would making
			// the dereferencing operation undefined behaviour as also
			// all operations on the reference.
			let tkn = unsafe { &mut *(tkn) };

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
		}
	}

	fn push_batch(
		&mut self,
		tkn_lst: Vec<TransferToken>,
	) -> (Vec<Pinned<TransferToken>>, usize, u8) {
		// Catch empty push, in order to allow zero initialized first_ctrl_settings struct
		// which will be overwritten in the first iteration of the for-loop
		assert!(tkn_lst.len() > 0);

		let mut first_ctrl_settings: (usize, u16, WrapCount) = (0, 0, WrapCount::new());
		let mut pind_lst = Vec::with_capacity(tkn_lst.len());

		for (i, tkn) in tkn_lst.into_iter().enumerate() {
			// fix memory address of token
			let mut pinned = Pinned::pin(tkn);

			// Check length and if its fits. This should always be true due to the restriction of
			// the memory pool, but to be sure.
			assert!(pinned.buff_tkn.as_ref().unwrap().num_consuming_descr() <= self.capacity);

			// create an counter that wrappes to the first element
			// after reaching a the end of the ring
			let mut ctrl = self.get_write_ctrler();

			// write the descriptors in reversed order into the queue. Starting with recv descriptors.
			// As the device MUST see all readable descriptors, bevore any writable descriptors
			// See Virtio specification v1.1. - 2.7.17
			//
			// Importance here is:
			// * distinguish between Indirect and direct buffers
			// * write descriptors in the correct order
			// * make them available in the right order (reversed order or i.e. lastly where device polls)
			match (
				&pinned.buff_tkn.as_ref().unwrap().send_buff,
				&pinned.buff_tkn.as_ref().unwrap().recv_buff,
			) {
				(Some(send_buff), Some(recv_buff)) => {
					// It is important to differentiate between indirect and direct descriptors here and if
					// send & recv descriptors are defined or only one of them.
					match (send_buff.get_ctrl_desc(), recv_buff.get_ctrl_desc()) {
						(Some(ctrl_desc), Some(_)) => {
							// One indirect descriptor with only flag indirect set
							ctrl.write_desc(ctrl_desc, DescrFlags::VIRTQ_DESC_F_INDIRECT.into());
						}
						(None, None) => {
							let mut buff_len =
								send_buff.as_slice().len() + recv_buff.as_slice().len();

							for desc in send_buff.as_slice() {
								if buff_len > 1 {
									ctrl.write_desc(desc, DescrFlags::VIRTQ_DESC_F_NEXT.into());
								} else {
									ctrl.write_desc(desc, 0);
								}
								buff_len -= 1;
							}

							for desc in recv_buff.as_slice() {
								if buff_len > 1 {
									ctrl.write_desc(
										desc,
										DescrFlags::VIRTQ_DESC_F_NEXT
											| DescrFlags::VIRTQ_DESC_F_WRITE,
									);
								} else {
									ctrl.write_desc(desc, DescrFlags::VIRTQ_DESC_F_WRITE.into());
								}
								buff_len -= 1;
							}
						}
						(None, Some(_)) => {
							unreachable!("Indirect buffers mixed with direct buffers!")
						} // This should already be catched at creation of BufferToken
						(Some(_), None) => {
							unreachable!("Indirect buffers mixed with direct buffers!")
						} // This should already be catched at creation of BufferToken,
					}
				}
				(Some(send_buff), None) => {
					match send_buff.get_ctrl_desc() {
						Some(ctrl_desc) => {
							// One indirect descriptor with only flag indirect set
							ctrl.write_desc(ctrl_desc, DescrFlags::VIRTQ_DESC_F_INDIRECT.into());
						}
						None => {
							let mut buff_len = send_buff.as_slice().len();

							for desc in send_buff.as_slice() {
								if buff_len > 1 {
									ctrl.write_desc(desc, DescrFlags::VIRTQ_DESC_F_NEXT.into());
								} else {
									ctrl.write_desc(desc, 0);
								}
								buff_len -= 1;
							}
						}
					}
				}
				(None, Some(recv_buff)) => {
					match recv_buff.get_ctrl_desc() {
						Some(ctrl_desc) => {
							// One indirect descriptor with only flag indirect set
							ctrl.write_desc(ctrl_desc, DescrFlags::VIRTQ_DESC_F_INDIRECT.into());
						}
						None => {
							let mut buff_len = recv_buff.as_slice().len();

							for desc in recv_buff.as_slice() {
								if buff_len > 1 {
									ctrl.write_desc(
										desc,
										DescrFlags::VIRTQ_DESC_F_NEXT
											| DescrFlags::VIRTQ_DESC_F_WRITE,
									);
								} else {
									ctrl.write_desc(desc, DescrFlags::VIRTQ_DESC_F_WRITE.into());
								}
								buff_len -= 1;
							}
						}
					}
				}
				(None, None) => unreachable!("Empty Transfers are not allowed!"), // This should already be catched at creation of BufferToken
			}

			if i == 0 {
				first_ctrl_settings = (ctrl.start, ctrl.buff_id, ctrl.wrap_at_init);
			} else {
				// Update flags of the first descriptor and set new write_index
				ctrl.make_avail(pinned.raw_addr());
			}

			// Update the state of the actual Token
			pinned.state = TransferState::Processing;
			pind_lst.push(pinned);
		}
		// Manually make the first buffer available lastly
		//
		// Providing the first buffer in the list manually
		// provide reference, in order to let TransferToken now upon finish.
		self.tkn_ref_ring[usize::try_from(first_ctrl_settings.1).unwrap()] = pind_lst[0].raw_addr();
		// The driver performs a suitable memory barrier to ensure the device sees the updated descriptor table and available ring before the next step.
		// See Virtio specfification v1.1. - 2.7.21
		fence(Ordering::SeqCst);
		self.ring[first_ctrl_settings.0].flags |= first_ctrl_settings.2.as_flags_avail();

		// Converting a boolean as u8 is fine
		(
			pind_lst,
			first_ctrl_settings.0,
			first_ctrl_settings.2 .0 as u8,
		)
	}

	fn push(&mut self, tkn: TransferToken) -> (Pinned<TransferToken>, usize, u8) {
		// fix memory address of token
		let mut pinned = Pinned::pin(tkn);

		// Check length and if its fits. This should always be true due to the restriction of
		// the memory pool, but to be sure.
		assert!(pinned.buff_tkn.as_ref().unwrap().num_consuming_descr() <= self.capacity);

		// create an counter that wrappes to the first element
		// after reaching a the end of the ring
		let mut ctrl = self.get_write_ctrler();

		// write the descriptors in reversed order into the queue. Starting with recv descriptors.
		// As the device MUST see all readable descriptors, bevore any writable descriptors
		// See Virtio specification v1.1. - 2.7.17
		//
		// Importance here is:
		// * distinguish between Indirect and direct buffers
		// * write descriptors in the correct order
		// * make them available in the right order (reversed order or i.e. lastly where device polls)
		match (
			&pinned.buff_tkn.as_ref().unwrap().send_buff,
			&pinned.buff_tkn.as_ref().unwrap().recv_buff,
		) {
			(Some(send_buff), Some(recv_buff)) => {
				// It is important to differentiate between indirect and direct descriptors here and if
				// send & recv descriptors are defined or only one of them.
				match (send_buff.get_ctrl_desc(), recv_buff.get_ctrl_desc()) {
					(Some(ctrl_desc), Some(_)) => {
						// One indirect descriptor with only flag indirect set
						ctrl.write_desc(ctrl_desc, DescrFlags::VIRTQ_DESC_F_INDIRECT.into());
					}
					(None, None) => {
						let mut buff_len = send_buff.as_slice().len() + recv_buff.as_slice().len();

						for desc in send_buff.as_slice() {
							if buff_len > 1 {
								ctrl.write_desc(desc, DescrFlags::VIRTQ_DESC_F_NEXT.into());
							} else {
								ctrl.write_desc(desc, 0);
							}
							buff_len -= 1;
						}

						for desc in recv_buff.as_slice() {
							if buff_len > 1 {
								ctrl.write_desc(
									desc,
									DescrFlags::VIRTQ_DESC_F_NEXT | DescrFlags::VIRTQ_DESC_F_WRITE,
								);
							} else {
								ctrl.write_desc(desc, DescrFlags::VIRTQ_DESC_F_WRITE.into());
							}
							buff_len -= 1;
						}
					}
					(None, Some(_)) => unreachable!("Indirect buffers mixed with direct buffers!"), // This should already be catched at creation of BufferToken
					(Some(_), None) => unreachable!("Indirect buffers mixed with direct buffers!"), // This should already be catched at creation of BufferToken,
				}
			}
			(Some(send_buff), None) => {
				match send_buff.get_ctrl_desc() {
					Some(ctrl_desc) => {
						// One indirect descriptor with only flag indirect set
						ctrl.write_desc(ctrl_desc, DescrFlags::VIRTQ_DESC_F_INDIRECT.into());
					}
					None => {
						let mut buff_len = send_buff.as_slice().len();

						for desc in send_buff.as_slice() {
							if buff_len > 1 {
								ctrl.write_desc(desc, DescrFlags::VIRTQ_DESC_F_NEXT.into());
							} else {
								ctrl.write_desc(desc, 0);
							}
							buff_len -= 1;
						}
					}
				}
			}
			(None, Some(recv_buff)) => {
				match recv_buff.get_ctrl_desc() {
					Some(ctrl_desc) => {
						// One indirect descriptor with only flag indirect set
						ctrl.write_desc(ctrl_desc, DescrFlags::VIRTQ_DESC_F_INDIRECT.into());
					}
					None => {
						let mut buff_len = recv_buff.as_slice().len();

						for desc in recv_buff.as_slice() {
							if buff_len > 1 {
								ctrl.write_desc(
									desc,
									DescrFlags::VIRTQ_DESC_F_NEXT | DescrFlags::VIRTQ_DESC_F_WRITE,
								);
							} else {
								ctrl.write_desc(desc, DescrFlags::VIRTQ_DESC_F_WRITE.into());
							}
							buff_len -= 1;
						}
					}
				}
			}
			(None, None) => unreachable!("Empty Transfers are not allowed!"), // This should already be catched at creation of BufferToken
		}

		fence(Ordering::SeqCst);
		// Update flags of the first descriptor and set new write_index
		ctrl.make_avail(pinned.raw_addr());
		fence(Ordering::SeqCst);

		// Update the state of the actual Token
		pinned.state = TransferState::Processing;

		// Converting a boolean as u8 is fine
		(pinned, ctrl.start, ctrl.wrap_at_init.0 as u8)
	}

	/// # Unsafe
	/// Returns the memory address of the first element of the descriptor ring
	fn raw_addr(&self) -> usize {
		self.ring.as_ptr() as usize
	}

	/// Returns an initialized write controler in order
	/// to write the queue correctly.
	fn get_write_ctrler(&mut self) -> WriteCtrl {
		WriteCtrl {
			start: self.write_index,
			position: self.write_index,
			modulo: self.ring.len(),
			wrap_at_init: self.drv_wc,
			buff_id: 0,

			desc_ring: self,
		}
	}

	/// Returns an initialized read controler in order
	/// to read the queue correctly.
	fn get_read_ctrler(&mut self) -> ReadCtrl {
		ReadCtrl {
			position: self.poll_index,
			modulo: self.ring.len(),

			desc_ring: self,
		}
	}
}

struct ReadCtrl<'a> {
	/// Poll index of the ring at init of ReadCtrl
	position: usize,
	modulo: usize,

	desc_ring: &'a mut DescriptorRing,
}

impl<'a> ReadCtrl<'a> {
	/// Polls the ring for a new finished buffer. If buffer is marked as used, takes care of
	/// updating the queue and returns the respective TransferToken.
	fn poll_next(&mut self) -> Option<*mut TransferToken> {
		// Check if descriptor has been marked used.
		if self.desc_ring.ring[self.position].flags & WrapCount::flag_mask()
			== self.desc_ring.dev_wc.as_flags_used()
		{
			let tkn;
			let recv_buff_opt;
			let send_buff_opt;

			unsafe {
				let raw_tkn = self.desc_ring.tkn_ref_ring
					[usize::try_from(self.desc_ring.ring[self.position].buff_id).unwrap()];
				assert!(!raw_tkn.is_null());
				tkn = &mut *(raw_tkn);

				// unset the reference in the refernce ring for security!
				self.desc_ring.tkn_ref_ring
					[usize::try_from(self.desc_ring.ring[self.position].buff_id).unwrap()] = 0 as *mut TransferToken;
				// This is perfectly fine, as we operate on two different datastructures inside one datastructure.
				let raw_ptr =
					(tkn.buff_tkn.as_ref().unwrap() as *const BufferToken) as *mut BufferToken;
				recv_buff_opt = &mut (*raw_ptr).recv_buff;
				send_buff_opt = &mut (*raw_ptr).send_buff;
			}

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
			let write_len = self.desc_ring.ring[self.position].len;

			match (send_buff_opt, recv_buff_opt) {
				(Some(send_buff), Some(recv_buff)) => {
					// Need to only check for either send or receive buff to contain
					// a ctrl_desc as, both carry the same if they carry one.
					if send_buff.is_indirect() {
						self.update_indirect(Some(send_buff), Some((recv_buff, write_len)));
					} else {
						self.update_send(send_buff);
						self.update_recv((recv_buff, write_len));
					}
				}
				(Some(send_buff), None) => {
					if send_buff.is_indirect() {
						self.update_indirect(Some(send_buff), None);
					} else {
						self.update_send(send_buff);
					}
				}
				(None, Some(recv_buff)) => {
					if recv_buff.is_indirect() {
						self.update_indirect(None, Some((recv_buff, write_len)));
					} else {
						self.update_recv((recv_buff, write_len));
					}
				}
				(None, None) => unreachable!("Empty Transfers are not allowed..."),
			}

			Some(tkn as *mut TransferToken)
		} else {
			None
		}
	}

	/// Updates the accesible len of the mempry areas accesible by the drivers to be consistend with
	/// the amount of data written by the device.
	///
	/// Indirect descriptor tables are read-only for devices. Hence all information comes from the
	/// used descriptor in the actual ring.
	fn update_indirect(
		&mut self,
		send_buff: Option<&mut Buffer>,
		recv_buff_spec: Option<(&mut Buffer, u32)>,
	) {
		match (send_buff, recv_buff_spec) {
			(Some(send_buff), Some((recv_buff, mut write_len))) => {
				// This is perfectly fine as we operate on two different datastructures inside one datastructure
				// we can have two mutable references via the same wrapping datastructure
				let ctrl_desc = unsafe {
					let raw_ref = &mut *((send_buff as *const Buffer) as *mut Buffer);
					raw_ref.get_ctrl_desc_mut().unwrap()
				};

				// This should read the descriptors inside the ctrl desc memory and update the memory
				// accordingly
				let desc_slice = unsafe {
					let size = core::mem::size_of::<Descriptor>();
					core::slice::from_raw_parts_mut(
						ctrl_desc.ptr as *mut Descriptor,
						ctrl_desc.len / size,
					)
				};

				let mut desc_iter = desc_slice.iter_mut();

				for desc in send_buff.as_mut_slice() {
					// Unwrapping is fine here, as lists must be of same size and same ordering
					desc_iter.next().unwrap();
				}

				recv_buff.restr_len(usize::try_from(write_len).unwrap());

				for desc in recv_buff.as_mut_slice() {
					// Unwrapping is fine here, as lists must be of same size and same ordering
					let ring_desc = desc_iter.next().unwrap();

					if write_len >= ring_desc.len {
						// Complete length has been written but reduce len_written for next one
						write_len -= ring_desc.len;
					} else {
						ring_desc.len = write_len;
						desc.len = write_len as usize;
						write_len -= ring_desc.len;
						assert_eq!(write_len, 0);
					}
				}
			}
			(Some(send_buff), None) => {
				// This is perfectly fine as we operate on two different datastructures inside one datastructure
				// we can have two mutable references via the same wrapping datastructure
				let ctrl_desc = unsafe {
					let raw_ref = &mut *((send_buff as *const Buffer) as *mut Buffer);
					raw_ref.get_ctrl_desc_mut().unwrap()
				};

				// This should read the descriptors inside the ctrl desc memory and update the memory
				// accordingly
				let desc_slice = unsafe {
					let size = core::mem::size_of::<Descriptor>();
					core::slice::from_raw_parts(
						ctrl_desc.ptr as *mut Descriptor,
						ctrl_desc.len / size,
					)
				};

				let mut desc_iter = desc_slice.into_iter();

				for desc in send_buff.as_mut_slice() {
					// Unwrapping is fine here, as lists must be of same size and same ordering
					desc_iter.next().unwrap();
				}
			}
			(None, Some((recv_buff, mut write_len))) => {
				// This is perfectly fine as we operate on two different datastructures inside one datastructure
				// we can have two mutable references via the same wrapping datastructure
				let ctrl_desc = unsafe {
					let raw_ref = &mut *((recv_buff as *const Buffer) as *mut Buffer);
					raw_ref.get_ctrl_desc_mut().unwrap()
				};

				// This should read the descriptors inside the ctrl desc memory and update the memory
				// accordingly
				let desc_slice = unsafe {
					let size = core::mem::size_of::<Descriptor>();
					core::slice::from_raw_parts_mut(
						ctrl_desc.ptr as *mut Descriptor,
						ctrl_desc.len / size,
					)
				};

				let mut desc_iter = desc_slice.iter_mut();

				recv_buff.restr_len(usize::try_from(write_len).unwrap());

				for desc in recv_buff.as_mut_slice() {
					// Unwrapping is fine here, as lists must be of same size and same ordering
					let ring_desc = desc_iter.next().unwrap();

					if write_len >= ring_desc.len {
						// Complete length has been written but reduce len_written for next one
						write_len -= ring_desc.len;
					} else {
						ring_desc.len = write_len;
						desc.len = write_len as usize;
						write_len -= ring_desc.len;
						assert_eq!(write_len, 0);
					}
				}
			}
			(None, None) => unreachable!("Empty transfers are not allowed."),
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
		self.desc_ring.ring[self.position].flags = self.desc_ring.dev_wc.as_flags_used();
	}

	/// Updates the accesible len of the mempry areas accesible by the drivers to be consistend with
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
	fn update_send(&mut self, send_buff: &mut Buffer) {
		for desc in send_buff.as_slice() {
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
		assert!(self.desc_ring.capacity <= self.desc_ring.ring.len());
		self.desc_ring.capacity += 1;

		self.desc_ring.poll_index = (self.desc_ring.poll_index + 1) % self.modulo;
		self.position = self.desc_ring.poll_index;
	}
}

/// Convenient struct that allows to convinently write descritpros into the queue.
/// The struct takes care of updating the state of the queue correctly and to write
/// the correct flags.
struct WriteCtrl<'a> {
	/// Where did the write of the buffer start in the descriptor ring
	/// This is important, as we must make this descriptor available
	/// lastly.
	start: usize,
	/// Where to write next. This should always be equal to the Rings
	/// write_next field.
	position: usize,
	modulo: usize,
	/// What was the WrapCount at the first write position
	/// Important in order to set the right avail and used flags
	wrap_at_init: WrapCount,
	/// Buff ID of this write
	buff_id: u16,

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
	fn write_desc(&mut self, mem_desc: &MemDescr, flags: u16) {
		// This also sets the buff_id for the WriteCtrl stuct to the ID of the first
		// descriptor.
		if self.start == self.position {
			let desc_ref = &mut self.desc_ring.ring[self.position];
			desc_ref.address = paging::virt_to_phys(VirtAddr::from(mem_desc.ptr as u64)).into();
			desc_ref.len = mem_desc.len as u32;
			desc_ref.buff_id = mem_desc.id.as_ref().unwrap().0;
			// Remove possibly set avail and used flags
			desc_ref.flags =
				flags & !(DescrFlags::VIRTQ_DESC_F_AVAIL) & !(DescrFlags::VIRTQ_DESC_F_USED);

			self.buff_id = mem_desc.id.as_ref().unwrap().0;
			self.incrmt();
		} else {
			let mut desc_ref = &mut self.desc_ring.ring[self.position];
			desc_ref.address = paging::virt_to_phys(VirtAddr::from(mem_desc.ptr as u64)).into();
			desc_ref.len = mem_desc.len as u32;
			desc_ref.buff_id = self.buff_id;
			// Remove possibly set avail and used flags and then set avail and used
			// according to the current WrapCount.
			desc_ref.flags =
				(flags & !(DescrFlags::VIRTQ_DESC_F_AVAIL) & !(DescrFlags::VIRTQ_DESC_F_USED))
					| self.desc_ring.drv_wc.as_flags_avail();

			self.incrmt()
		}
	}

	fn make_avail(&mut self, raw_tkn: *mut TransferToken) {
		// We fail if one wants to make a buffer availbale without inserting one element!
		assert!(self.start != self.position);
		// We also fail if buff_id is not set!
		assert!(self.buff_id != 0);

		// provide reference, in order to let TransferToken now upon finish.
		self.desc_ring.tkn_ref_ring[usize::try_from(self.buff_id).unwrap()] = raw_tkn;
		// The driver performs a suitable memory barrier to ensure the device sees the updated descriptor table and available ring before the next step.
		// See Virtio specfification v1.1. - 2.7.21
		fence(Ordering::SeqCst);
		self.desc_ring.ring[self.start].flags |= self.wrap_at_init.as_flags_avail();
	}
}

#[repr(C, align(16))]
struct Descriptor {
	address: u64,
	len: u32,
	buff_id: u16,
	flags: u16,
}

impl Descriptor {
	fn new(add: u64, len: u32, id: u16, flags: u16) -> Self {
		Descriptor {
			address: add,
			len,
			buff_id: id,
			flags,
		}
	}

	fn to_le_bytes(self) -> [u8; 16] {
		let mut desc_bytes_cnt = 0usize;
		// 128 bits long raw descriptor bytes
		let mut desc_bytes: [u8; 16] = [0; 16];

		// Call to little endian, as device will read this and
		// Virtio devices are inherently little endian coded.
		let mem_addr: [u8; 8] = self.address.to_le_bytes();
		// Write address as bytes in raw
		for byte in 0..8 {
			desc_bytes[desc_bytes_cnt] = mem_addr[byte];
			desc_bytes_cnt += 1;
		}

		// Must be 32 bit in order to fulfill specification.
		// MemPool.pull and .pull_untracked ensure this automatically
		// which makes this cast safe.
		let mem_len: [u8; 4] = self.len.to_le_bytes();
		// Write length of memory area as bytes in raw
		for byte in 0..4 {
			desc_bytes[desc_bytes_cnt] = mem_len[byte];
			desc_bytes_cnt += 1;
		}

		// Write BuffID as bytes in raw.
		let id: [u8; 2] = self.buff_id.to_le_bytes();
		for byte in 0..2usize {
			desc_bytes[desc_bytes_cnt] = id[byte];
			desc_bytes_cnt += 1;
		}

		// Write flags as bytes in raw.
		let flags: [u8; 2] = self.flags.to_le_bytes();
		// Write of flags as bytes in raw
		for byte in 0..2usize {
			desc_bytes[desc_bytes_cnt] = flags[byte];
		}

		desc_bytes
	}
}

/// Driver and device event suppression struct used in packed virtqueues.
///
/// Structure layout see Virtio specification v1.1. - 2.7.14
/// Alignment see Virtio specification v1.1. - 2.7.10.1
///
// /* Enable events */
// #define RING_EVENT_FLAGS_ENABLE 0x0
// /* Disable events */
// #define RING_EVENT_FLAGS_DISABLE 0x1
// /*
//  * Enable events for a specific descriptor
//  * (as specified by Descriptor Ring Change Event Offset/Wrap Counter). * Only valid if VIRTIO_F_RING_EVENT_IDX has been negotiated.
//  */
//  #define RING_EVENT_FLAGS_DESC 0x2
//  /* The value 0x3 is reserved */
//
// struct pvirtq_event_suppress {
//      le16 {
//         desc_event_off : 15;     /* Descriptor Ring Change Event Offset */
//         desc_event_wrap : 1;     /* Descriptor Ring Change Event Wrap Counter */
//      } desc;                     /* If desc_event_flags set to RING_EVENT_FLAGS_DESC */ -> For a single descriptor notification settings
//      le16 {
//         desc_event_flags : 2,    /* Descriptor Ring Change Event Flags */ -> General notification on/off
//         reserved : 14;           /* Reserved, set to 0 */
//      } flags;
// };
#[repr(C, align(4))]
struct EventSuppr {
	event: u16,
	flags: u16,
}

/// A newtype in order to implement the correct functionality upon
/// the `EventSuppr` structure for driver notifications settings.
/// The Driver Event Suppression structure is read-only by the device
/// and controls the used buffer notifications sent by the device to the driver.
struct DrvNotif {
	/// Indicates if VIRTIO_F_RING_EVENT_IDX has been negotiated
	f_notif_idx: bool,
	/// Actual structure to read from, if device wants notifs
	raw: &'static mut EventSuppr,
}

/// A newtype in order to implement the correct functionality upon
/// the `EventSuppr` structure for device notifications settings.
/// The Device Event Suppression structure is read-only by the driver
/// and controls the available buffer notifica- tions sent by the driver to the device.
struct DevNotif {
	/// Indicates if VIRTIO_F_RING_EVENT_IDX has been negotiated
	f_notif_idx: bool,
	/// Actual structure to read from, if device wants notifs
	raw: &'static mut EventSuppr,
}

impl EventSuppr {
	/// Returns a zero initialized EventSuppr structure
	fn new() -> Self {
		EventSuppr { event: 0, flags: 0 }
	}
}

impl DrvNotif {
	/// Enables notifications by setting the LSB.
	/// See Virito specification v1.1. - 2.7.10
	fn enable_notif(&mut self) {
		self.raw.flags = 0;
	}

	/// Disables notifications by unsetting the LSB.
	/// See Virtio specification v1.1. - 2.7.10
	fn disable_notif(&mut self) {
		self.raw.flags = 0;
	}

	/// Enables a notification by the device for a specific descriptor.
	fn enable_specific(&mut self, at_offset: u16, at_wrap: u8) {
		// Check if VIRTIO_F_RING_EVENT_IDX has been negotiated
		if self.f_notif_idx {
			self.raw.flags |= 1 << 1;
			// Reset event fields
			self.raw.event = 0;
			self.raw.event = at_offset;
			self.raw.event |= (at_wrap as u16) << 15;
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
		self.raw.flags & (1 << 0) == 0
	}

	fn is_notif_specfic(&self, next_off: usize, next_wrap: u8) -> bool {
		if self.f_notif_idx {
			if self.raw.flags & 1 << 1 == 2 {
				// as u16 is okay for usize, as size of queue is restricted to 2^15
				// it is also okay to just loose the upper 8 bits, as we only check the LSB in second clause.
				let desc_event_off = self.raw.event & !(1 << 15);
				let desc_event_wrap = (self.raw.event >> 15) as u8;

				desc_event_off == next_off as u16 && desc_event_wrap == next_wrap as u8
			} else {
				false
			}
		} else {
			false
		}
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
	/// Memory pool controls the amount of "free floating" descriptors
	/// See [MemPool](super.MemPool) docs for detail.
	mem_pool: Rc<MemPool>,
	/// The size of the queue, equals the number of descriptors which can
	/// be used
	size: VqSize,
	/// The virtqueues index. This identifies the virtqueue to the
	/// device and is unique on a per device basis
	index: VqIndex,
	/// Holds all erly dropped `TransferToken`
	/// If `TransferToken.state == TransferState::Finished`
	/// the Token can be safely dropped
	dropped: RefCell<Vec<Pinned<TransferToken>>>,
}

// Public interface of PackedVq
// This interface is also public in order to allow people to use the PackedVq directly!
// This is currently unlikely, as the Tokens hold a Rc<Virtq> for refering to their origin
// queue. This could be eased
impl PackedVq {
	/// Enables interrupts for this virtqueue upon receiving a transfer
	pub fn enable_notifs(&self) {
		self.drv_event.borrow_mut().enable_notif();
	}

	/// Disables interrupts for this virtqueue upon receiving a transfer
	pub fn disable_notifs(&self) {
		self.drv_event.borrow_mut().disable_notif();
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
		self.descr_ring.borrow_mut().poll();
	}

	/// Dispatches a batch of transfer token. The buffers of the respective transfers are provided to the queue in
	/// sequence. After the last buffer has been writen, the queue marks the first buffer as available and triggers
	/// a device notification if wanted by the device.
	///
	/// The `notif` parameter indicates if the driver wants to have a notification for this specific
	/// transfer. This is only for performance optimization. As it is NOT ensured, that the device sees the
	/// updated notification flags before finishing transfers!
	pub fn dispatch_batch(&self, tkns: Vec<TransferToken>, notif: bool) -> Vec<Transfer> {
		// Zero transfers are not allowed
		assert!(tkns.len() > 0);

		let (pin_tkn_lst, next_off, next_wrap) = self.descr_ring.borrow_mut().push_batch(tkns);

		if notif {
			self.drv_event
				.borrow_mut()
				.enable_specific(next_off as u16, next_wrap);
		}

		if self.dev_event.is_notif() | self.dev_event.is_notif_specfic(next_off, next_wrap) {
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

		let mut transfer_lst = Vec::with_capacity(pin_tkn_lst.len());

		for pinned in pin_tkn_lst {
			transfer_lst.push(Transfer {
				transfer_tkn: Some(pinned),
			})
		}

		transfer_lst
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
		mut tkns: Vec<TransferToken>,
		await_queue: Rc<RefCell<VecDeque<Transfer>>>,
		notif: bool,
	) {
		// Zero transfers are not allowed
		assert!(tkns.len() > 0);

		// We have to iterate here too, in order to ensure, tokens are placed into the await_queue
		for tkn in tkns.iter_mut() {
			tkn.await_queue = Some(Rc::clone(&await_queue));
		}

		let (pin_tkn_lst, next_off, next_wrap) = self.descr_ring.borrow_mut().push_batch(tkns);

		if notif {
			self.drv_event
				.borrow_mut()
				.enable_specific(next_off as u16, next_wrap);
		}

		if self.dev_event.is_notif() {
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

		for pinned in pin_tkn_lst {
			// Prevent TransferToken from beeing dropped
			// I.e. do NOT run the costum constructor which will
			// deallocate memory.
			pinned.into_raw();
		}
	}

	/// See `Virtq.prep_transfer()` documentation.
	///
	/// The `notif` parameter indicates if the driver wants to have a notification for this specific
	/// transfer. This is only for performance optimization. As it is NOT ensured, that the device sees the
	/// updated notification flags before finishing transfers!
	pub fn dispatch(&self, tkn: TransferToken, notif: bool) -> Transfer {
		let (pin_tkn, next_off, next_wrap) = self.descr_ring.borrow_mut().push(tkn);

		if notif {
			self.drv_event
				.borrow_mut()
				.enable_specific(next_off as u16, next_wrap);
		}

		if self.dev_event.is_notif() {
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
	) -> Result<Self, VqPackedError> {
		// Currently we do not have support for in order use.
		// This steems from the fact, that the packedVq ReadCtrl currently is not
		// able to derive other finished transfer from a used-buffer notification.
		// In order to allow this, the queue MUST track the sequence in which
		// TransferTokens are inserted into the queue. Furthermore the Queu should
		// carry a feature u64 in order to check which features are used currently
		// and adjust its ReadCtrl accordingly.
		if feats & Features::VIRTIO_F_IN_ORDER == Features::VIRTIO_F_IN_ORDER {
			info!("PackedVq has no support for VIRTIO_F_IN_ORDER. Aborting...");
			return Err(VqPackedError::FeatNotSupported(
				feats & Features::VIRTIO_F_IN_ORDER,
			));
		}

		// Get a handler to the queues configuration area.
		let mut vq_handler = match com_cfg.select_vq(index.into()) {
			Some(handler) => handler,
			None => return Err(VqPackedError::QueueNotExisting(index.into())),
		};

		// Must catch zero size as it is not allowed for packed queues.
		// Must catch size larger 32768 (2^15) as it is not allowed for packed queues.
		//
		// See Virtio specification v1.1. - 4.1.4.3.2
		let vq_size = if (size.0 == 0) | (size.0 > 32768) {
			return Err(VqPackedError::SizeNotAllowed(size.0));
		} else {
			vq_handler.set_vq_size(size.0)
		};

		let descr_ring = RefCell::new(DescriptorRing::new(vq_size));
		// Allocate heap memory via a vec, leak and cast
		let _mem_len = align_up!(core::mem::size_of::<EventSuppr>(), BasePageSize::SIZE);

		let drv_event_ptr =
			(crate::mm::allocate(_mem_len, true).0 as *const EventSuppr) as *mut EventSuppr;
		let dev_event_ptr =
			(crate::mm::allocate(_mem_len, true).0 as *const EventSuppr) as *mut EventSuppr;

		// Provide memory areas of the queues data structures to the device
		vq_handler.set_ring_addr(paging::virt_to_phys(VirtAddr::from(
			descr_ring.borrow().raw_addr() as u64,
		)));
		// As usize is safe here, as the *mut EventSuppr raw pointer is a thin pointer of size usize
		vq_handler.set_drv_ctrl_addr(paging::virt_to_phys(VirtAddr::from(drv_event_ptr as u64)));
		vq_handler.set_dev_ctrl_addr(paging::virt_to_phys(VirtAddr::from(dev_event_ptr as u64)));

		let drv_event: &'static mut EventSuppr = unsafe { &mut *(drv_event_ptr) };

		let dev_event: &'static mut EventSuppr = unsafe { &mut *(dev_event_ptr) };

		let drv_event = RefCell::new(DrvNotif {
			f_notif_idx: false,
			raw: drv_event,
		});

		let dev_event = DevNotif {
			f_notif_idx: false,
			raw: dev_event,
		};

		let mut notif_ctrl = NotifCtrl::new(
			(notif_cfg.base()
				+ usize::try_from(vq_handler.notif_off()).unwrap()
				+ usize::try_from(notif_cfg.multiplier()).unwrap()) as *mut usize,
		);

		if feats & Features::VIRTIO_F_NOTIFICATION_DATA == Features::VIRTIO_F_NOTIFICATION_DATA {
			notif_ctrl.enable_notif_data();
		}

		if feats & Features::VIRTIO_F_RING_EVENT_IDX == Features::VIRTIO_F_RING_EVENT_IDX {
			drv_event.borrow_mut().f_notif_idx = true;
		}

		// Initialize new memory pool.
		let mem_pool = Rc::new(MemPool::new(vq_size));

		// Initialize an empty vector for future dropped transfers
		let dropped: RefCell<Vec<Pinned<TransferToken>>> = RefCell::new(Vec::new());

		vq_handler.enable_queue();

		info!("Created PackedVq: idx={}, size={}", index.0, vq_size);

		Ok(PackedVq {
			descr_ring,
			drv_event,
			dev_event,
			notif_ctrl,
			mem_pool,
			size: VqSize::from(vq_size),
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
impl PackedVq {
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

		let desc_slice = unsafe {
			let size = core::mem::size_of::<Descriptor>();
			core::slice::from_raw_parts_mut(ctrl_desc.ptr as *mut Descriptor, ctrl_desc.len / size)
		};

		match (send, recv) {
			(None, None) => return Err(VirtqError::BufferNotSpecified),
			// Only recving descriptorsn (those are writabel by device)
			(None, Some(recv_desc_lst)) => {
				for desc in recv_desc_lst {
					desc_slice[crtl_desc_iter] = Descriptor::new(
						paging::virt_to_phys(VirtAddr::from(desc.ptr as u64)).into(),
						desc.len as u32,
						0,
						DescrFlags::VIRTQ_DESC_F_WRITE.into(),
					);

					crtl_desc_iter += 1;
				}
				Ok(ctrl_desc)
			}
			// Only sending descritpors
			(Some(send_desc_lst), None) => {
				for desc in send_desc_lst {
					desc_slice[crtl_desc_iter] = Descriptor::new(
						paging::virt_to_phys(VirtAddr::from(desc.ptr as u64)).into(),
						desc.len as u32,
						0,
						0,
					);

					crtl_desc_iter += 1;
				}
				Ok(ctrl_desc)
			}
			(Some(send_desc_lst), Some(recv_desc_lst)) => {
				// Send descriptors ALWAYS before receiving ones.
				for desc in send_desc_lst {
					desc_slice[crtl_desc_iter] = Descriptor::new(
						paging::virt_to_phys(VirtAddr::from(desc.ptr as u64)).into(),
						desc.len as u32,
						0,
						0,
					);

					crtl_desc_iter += 1;
				}

				for desc in recv_desc_lst {
					desc_slice[crtl_desc_iter] = Descriptor::new(
						paging::virt_to_phys(VirtAddr::from(desc.ptr as u64)).into(),
						desc.len as u32,
						0,
						DescrFlags::VIRTQ_DESC_F_WRITE.into(),
					);

					crtl_desc_iter += 1;
				}

				Ok(ctrl_desc)
			}
		}
	}
}

pub mod error {
	pub enum VqPackedError {
		General,
		SizeNotAllowed(u16),
		QueueNotExisting(u16),
		FeatNotSupported(u64),
	}
}
