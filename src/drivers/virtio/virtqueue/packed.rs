//! This module contains Virtio's packed virtqueue.
//! See Virito specification v1.1. - 2.7
#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::ptr;
use core::sync::atomic::{fence, Ordering};

use align_address::Align;
use zerocopy::little_endian;

#[cfg(not(feature = "pci"))]
use super::super::transport::mmio::{ComCfg, NotifCfg, NotifCtrl};
#[cfg(feature = "pci")]
use super::super::transport::pci::{ComCfg, NotifCfg, NotifCtrl};
use super::error::VirtqError;
use super::{
	BuffSpec, Buffer, BufferToken, BufferType, Bytes, DescrFlags, MemDescr, MemPool, TransferToken,
	Virtq, VirtqPrivate, VqIndex, VqSize,
};
use crate::arch::mm::paging::{BasePageSize, PageSize};
use crate::arch::mm::{paging, VirtAddr};

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
		self.0 = !self.0
	}

	/// Creates avail and used flags inside u16 in accordance to the
	/// virito specification v1.1. - 2.7.1
	///
	/// I.e.: Set avail flag to match the WrapCount and the used flag
	/// to NOT match the WrapCount.
	fn as_flags_avail(&self) -> u16 {
		if self.0 {
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
		if self.0 {
			1 << 7 | 1 << 15
		} else {
			0
		}
	}
}

/// Structure which allows to control raw ring and operate easily on it
struct DescriptorRing {
	ring: &'static mut [Descriptor],
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
}

impl DescriptorRing {
	fn new(size: u16) -> Self {
		// Allocate heap memory via a vec, leak and cast
		let _mem_len = (usize::from(size) * core::mem::size_of::<Descriptor>())
			.align_up(BasePageSize::SIZE as usize);
		let ptr = ptr::with_exposed_provenance_mut(crate::mm::allocate(_mem_len, true).0 as usize);

		let ring: &'static mut [Descriptor] =
			unsafe { core::slice::from_raw_parts_mut(ptr, size.into()) };

		// Descriptor ID's run from 1 to size_of_queue. In order to index directly into the
		// reference ring via an ID it is much easier to simply have an array of size = size_of_queue + 1
		// and do not care about the first element being unused.
		// `Box` is not Clone, so neither is `None::<Box<_>>`. Hence, we need to produce `None`s with a closure.
		let tkn_ref_ring = core::iter::repeat_with(|| None)
			.take((size + 1).into())
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
		}
	}

	/// Polls poll index and sets the state of any finished TransferTokens.
	/// If [TransferToken::await_queue] is available, the [BufferToken] will be moved to the queue.
	fn poll(&mut self) {
		let mut ctrl = self.get_read_ctrler();

		if let Some(mut tkn) = ctrl.poll_next() {
			if let Some(queue) = tkn.await_queue.take() {
				// Place the TransferToken in a Transfer, which will hold ownership of the token
				queue.try_send(Box::new(tkn.buff_tkn.unwrap())).unwrap();
			}
		}
	}

	fn push_batch(&mut self, tkn_lst: Vec<TransferToken>) -> (u16, u8) {
		// Catch empty push, in order to allow zero initialized first_ctrl_settings struct
		// which will be overwritten in the first iteration of the for-loop
		assert!(!tkn_lst.is_empty());

		let mut first_ctrl_settings: (u16, u16, WrapCount) = (0, 0, WrapCount::new());
		let mut first_buffer = None;

		for (i, tkn) in tkn_lst.into_iter().enumerate() {
			// Check length and if its fits. This should always be true due to the restriction of
			// the memory pool, but to be sure.
			assert!(tkn.buff_tkn.as_ref().unwrap().num_consuming_descr() <= self.capacity);

			// create an counter that wrappes to the first element
			// after reaching a the end of the ring
			let mut ctrl = self.get_write_ctrler();

			// write the descriptors in reversed order into the queue. Starting with recv descriptors.
			// As the device MUST see all readable descriptors, before any writable descriptors
			// See Virtio specification v1.1. - 2.7.17
			//
			// Importance here is:
			// * distinguish between Indirect and direct buffers
			// * write descriptors in the correct order
			// * make them available in the right order (reversed order or i.e. lastly where device polls)
			match (
				&tkn.buff_tkn.as_ref().unwrap().send_buff,
				&tkn.buff_tkn.as_ref().unwrap().recv_buff,
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
						} // This should already be caught at creation of BufferToken
						(Some(_), None) => {
							unreachable!("Indirect buffers mixed with direct buffers!")
						} // This should already be caught at creation of BufferToken,
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
				(None, None) => unreachable!("Empty Transfers are not allowed!"), // This should already be caught at creation of BufferToken
			}

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
		// provide reference, in order to let TransferToken now upon finish.
		self.tkn_ref_ring[usize::from(first_ctrl_settings.1)] = first_buffer;
		// The driver performs a suitable memory barrier to ensure the device sees the updated descriptor table and available ring before the next step.
		// See Virtio specfification v1.1. - 2.7.21
		fence(Ordering::SeqCst);
		self.ring[usize::from(first_ctrl_settings.0)].flags |=
			first_ctrl_settings.2.as_flags_avail().into();

		// Converting a boolean as u8 is fine
		(first_ctrl_settings.0, first_ctrl_settings.2 .0 as u8)
	}

	fn push(&mut self, tkn: TransferToken) -> (u16, u8) {
		// Check length and if its fits. This should always be true due to the restriction of
		// the memory pool, but to be sure.
		assert!(tkn.buff_tkn.as_ref().unwrap().num_consuming_descr() <= self.capacity);

		// create an counter that wrappes to the first element
		// after reaching a the end of the ring
		let mut ctrl = self.get_write_ctrler();

		// write the descriptors in reversed order into the queue. Starting with recv descriptors.
		// As the device MUST see all readable descriptors, before any writable descriptors
		// See Virtio specification v1.1. - 2.7.17
		//
		// Importance here is:
		// * distinguish between Indirect and direct buffers
		// * write descriptors in the correct order
		// * make them available in the right order (reversed order or i.e. lastly where device polls)
		match (
			&tkn.buff_tkn.as_ref().unwrap().send_buff,
			&tkn.buff_tkn.as_ref().unwrap().recv_buff,
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
					(None, Some(_)) => unreachable!("Indirect buffers mixed with direct buffers!"), // This should already be caught at creation of BufferToken
					(Some(_), None) => unreachable!("Indirect buffers mixed with direct buffers!"), // This should already be caught at creation of BufferToken,
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
			(None, None) => unreachable!("Empty Transfers are not allowed!"), // This should already be caught at creation of BufferToken
		}

		fence(Ordering::SeqCst);
		// Update flags of the first descriptor and set new write_index
		ctrl.make_avail(Box::new(tkn));
		fence(Ordering::SeqCst);

		// Converting a boolean as u8 is fine
		(ctrl.start, ctrl.wrap_at_init.0 as u8)
	}

	/// # Unsafe
	/// Returns the memory address of the first element of the descriptor ring
	fn raw_addr(&self) -> usize {
		self.ring.as_ptr() as usize
	}

	/// Returns an initialized write controller in order
	/// to write the queue correctly.
	fn get_write_ctrler(&mut self) -> WriteCtrl<'_> {
		WriteCtrl {
			start: self.write_index,
			position: self.write_index,
			modulo: u16::try_from(self.ring.len()).unwrap(),
			wrap_at_init: self.drv_wc,
			buff_id: 0,

			desc_ring: self,
		}
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
		if self.desc_ring.ring[usize::from(self.position)].flags.get() & WrapCount::flag_mask()
			== self.desc_ring.dev_wc.as_flags_used()
		{
			let buff_id = usize::from(self.desc_ring.ring[usize::from(self.position)].buff_id);
			let mut tkn = self.desc_ring.tkn_ref_ring[buff_id].take().expect(
				"The buff_id is incorrect or the reference to the TransferToken was misplaced.",
			);

			let (send_buff, recv_buff) = {
				let BufferToken {
					send_buff,
					recv_buff,
					..
				} = tkn.buff_tkn.as_mut().unwrap();
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

			match (send_buff, recv_buff) {
				(Some(send_buff), Some(recv_buff)) => {
					// Need to only check for either send or receive buff to contain
					// a ctrl_desc as, both carry the same if they carry one.
					if send_buff.is_indirect() {
						self.update_indirect(Some(send_buff), Some((recv_buff, write_len.into())));
					} else {
						self.update_send(send_buff);
						self.update_recv((recv_buff, write_len.into()));
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
						self.update_indirect(None, Some((recv_buff, write_len.into())));
					} else {
						self.update_recv((recv_buff, write_len.into()));
					}
				}
				(None, None) => unreachable!("Empty Transfers are not allowed..."),
			}

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
	fn update_indirect(
		&mut self,
		send_buff: Option<&mut Buffer>,
		recv_buff_spec: Option<(&mut Buffer, u32)>,
	) {
		match (send_buff, recv_buff_spec) {
			(Some(send_buff), Some((recv_buff, mut write_len))) => {
				let ctrl_desc = send_buff.get_ctrl_desc_mut().unwrap();

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

				for _desc in send_buff.as_mut_slice() {
					// Unwrapping is fine here, as lists must be of same size and same ordering
					desc_iter.next().unwrap();
				}

				recv_buff.restr_len(usize::try_from(write_len).unwrap());

				for desc in recv_buff.as_mut_slice() {
					// Unwrapping is fine here, as lists must be of same size and same ordering
					let ring_desc = desc_iter.next().unwrap();

					if write_len >= ring_desc.len.into() {
						// Complete length has been written but reduce len_written for next one
						write_len -= ring_desc.len.get();
					} else {
						ring_desc.len = (write_len).into();
						desc.len = write_len as usize;
						write_len -= ring_desc.len.get();
						assert_eq!(write_len, 0);
					}
				}
			}
			(Some(send_buff), None) => {
				let ctrl_desc = send_buff.get_ctrl_desc_mut().unwrap();

				// This should read the descriptors inside the ctrl desc memory and update the memory
				// accordingly
				let desc_slice = unsafe {
					let size = core::mem::size_of::<Descriptor>();
					core::slice::from_raw_parts(
						ctrl_desc.ptr as *mut Descriptor,
						ctrl_desc.len / size,
					)
				};

				let mut desc_iter = desc_slice.iter();

				for _desc in send_buff.as_mut_slice() {
					// Unwrapping is fine here, as lists must be of same size and same ordering
					desc_iter.next().unwrap();
				}
			}
			(None, Some((recv_buff, mut write_len))) => {
				let ctrl_desc = recv_buff.get_ctrl_desc_mut().unwrap();

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

					if write_len >= ring_desc.len.into() {
						// Complete length has been written but reduce len_written for next one
						write_len -= ring_desc.len.get();
					} else {
						ring_desc.len = write_len.into();
						desc.len = write_len as usize;
						write_len -= ring_desc.len.get();
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
		self.desc_ring.ring[usize::from(self.position)].flags =
			self.desc_ring.dev_wc.as_flags_used().into();
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
		// This also sets the buff_id for the WriteCtrl struct to the ID of the first
		// descriptor.
		if self.start == self.position {
			let desc_ref = &mut self.desc_ring.ring[usize::from(self.position)];
			desc_ref
				.address
				.set(paging::virt_to_phys(VirtAddr::from(mem_desc.ptr as u64)).into());
			desc_ref.len = (mem_desc.len as u32).into();
			desc_ref.buff_id = (mem_desc.id.as_ref().unwrap().0).into();
			// Remove possibly set avail and used flags
			desc_ref.flags =
				(flags & !(DescrFlags::VIRTQ_DESC_F_AVAIL) & !(DescrFlags::VIRTQ_DESC_F_USED))
					.into();

			self.buff_id = mem_desc.id.as_ref().unwrap().0;
			self.incrmt();
		} else {
			let desc_ref = &mut self.desc_ring.ring[usize::from(self.position)];
			desc_ref
				.address
				.set(paging::virt_to_phys(VirtAddr::from(mem_desc.ptr as u64)).into());
			desc_ref.len = (mem_desc.len as u32).into();
			desc_ref.buff_id = (self.buff_id).into();
			// Remove possibly set avail and used flags and then set avail and used
			// according to the current WrapCount.
			desc_ref.flags =
				((flags & !(DescrFlags::VIRTQ_DESC_F_AVAIL) & !(DescrFlags::VIRTQ_DESC_F_USED))
					| self.desc_ring.drv_wc.as_flags_avail())
				.into();

			self.incrmt()
		}
	}

	fn make_avail(&mut self, raw_tkn: Box<TransferToken>) {
		// We fail if one wants to make a buffer available without inserting one element!
		assert!(self.start != self.position);
		// We also fail if buff_id is not set!
		assert!(self.buff_id != 0);

		// provide reference, in order to let TransferToken know upon finish.
		self.desc_ring.tkn_ref_ring[usize::from(self.buff_id)] = Some(raw_tkn);
		// The driver performs a suitable memory barrier to ensure the device sees the updated descriptor table and available ring before the next step.
		// See Virtio specfification v1.1. - 2.7.21
		fence(Ordering::SeqCst);
		self.desc_ring.ring[usize::from(self.start)].flags |=
			self.wrap_at_init.as_flags_avail().into();
	}
}

#[derive(Clone, Copy)]
#[repr(C, align(16))]
struct Descriptor {
	address: little_endian::U64,
	len: little_endian::U32,
	buff_id: little_endian::U16,
	flags: little_endian::U16,
}

impl Descriptor {
	fn new(add: u64, len: u32, id: u16, flags: u16) -> Self {
		Descriptor {
			address: add.into(),
			len: len.into(),
			buff_id: id.into(),
			flags: flags.into(),
		}
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

	fn is_notif_specfic(&self, next_off: u16, next_wrap: u8) -> bool {
		if self.f_notif_idx {
			if self.raw.flags & 1 << 1 == 2 {
				// as u16 is okay for usize, as size of queue is restricted to 2^15
				// it is also okay to just loose the upper 8 bits, as we only check the LSB in second clause.
				let desc_event_off = self.raw.event & !(1 << 15);
				let desc_event_wrap = (self.raw.event >> 15) as u8;

				desc_event_off == next_off && desc_event_wrap == next_wrap
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
	/// See [MemPool] docs for detail.
	mem_pool: Rc<MemPool>,
	/// The size of the queue, equals the number of descriptors which can
	/// be used
	size: VqSize,
	/// The virtqueues index. This identifies the virtqueue to the
	/// device and is unique on a per device basis.
	index: VqIndex,
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

	fn dispatch_batch(&self, tkns: Vec<TransferToken>, notif: bool) {
		// Zero transfers are not allowed
		assert!(!tkns.is_empty());

		let (next_off, next_wrap) = self.descr_ring.borrow_mut().push_batch(tkns);

		if notif {
			self.drv_event
				.borrow_mut()
				.enable_specific(next_off, next_wrap);
		}

		if self.dev_event.is_notif() | self.dev_event.is_notif_specfic(next_off, next_wrap) {
			let index = self.index.0.to_le_bytes();
			let mut index = index.iter();
			let det_notif_data: u16 = next_off & !(1 << 15);
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
	}

	fn dispatch_batch_await(
		&self,
		mut tkns: Vec<TransferToken>,
		await_queue: super::BufferTokenSender,
		notif: bool,
	) {
		// Zero transfers are not allowed
		assert!(!tkns.is_empty());

		// We have to iterate here too, in order to ensure, tokens are placed into the await_queue
		for tkn in tkns.iter_mut() {
			tkn.await_queue = Some(await_queue.clone());
		}

		let (next_off, next_wrap) = self.descr_ring.borrow_mut().push_batch(tkns);

		if notif {
			self.drv_event
				.borrow_mut()
				.enable_specific(next_off, next_wrap);
		}

		if self.dev_event.is_notif() {
			let index = self.index.0.to_le_bytes();
			let mut index = index.iter();
			let det_notif_data: u16 = next_off & !(1 << 15);
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
	}

	fn dispatch(&self, tkn: TransferToken, notif: bool) {
		let (next_off, next_wrap) = self.descr_ring.borrow_mut().push(tkn);

		if notif {
			self.drv_event
				.borrow_mut()
				.enable_specific(next_off, next_wrap);
		}

		if self.dev_event.is_notif() {
			let index = self.index.0.to_le_bytes();
			let mut index = index.iter();
			let det_notif_data: u16 = next_off & !(1 << 15);
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
	}

	fn index(&self) -> VqIndex {
		self.index
	}

	fn new(
		com_cfg: &mut ComCfg,
		notif_cfg: &NotifCfg,
		size: VqSize,
		index: VqIndex,
		features: virtio_spec::F,
	) -> Result<Self, VirtqError> {
		// Currently we do not have support for in order use.
		// This steems from the fact, that the packedVq ReadCtrl currently is not
		// able to derive other finished transfer from a used-buffer notification.
		// In order to allow this, the queue MUST track the sequence in which
		// TransferTokens are inserted into the queue. Furthermore the Queue should
		// carry a feature u64 in order to check which features are used currently
		// and adjust its ReadCtrl accordingly.
		if features.contains(virtio_spec::F::IN_ORDER) {
			info!("PackedVq has no support for VIRTIO_F_IN_ORDER. Aborting...");
			return Err(VirtqError::FeatureNotSupported(virtio_spec::F::IN_ORDER));
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
		let _mem_len = core::mem::size_of::<EventSuppr>().align_up(BasePageSize::SIZE as usize);

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

		let mut notif_ctrl = NotifCtrl::new(notif_cfg.notification_location(&mut vq_handler));

		if features.contains(virtio_spec::F::NOTIFICATION_DATA) {
			notif_ctrl.enable_notif_data();
		}

		if features.contains(virtio_spec::F::EVENT_IDX) {
			drv_event.borrow_mut().f_notif_idx = true;
		}

		// Initialize new memory pool.
		let mem_pool = Rc::new(MemPool::new(vq_size));

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
		})
	}

	fn prep_transfer_from_raw(
		self: Rc<Self>,
		send: &[&[u8]],
		recv: &[&mut [u8]],
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
	fn mem_pool(&self) -> Rc<MemPool> {
		self.mem_pool.clone()
	}

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

		let ctrl_desc = match self.mem_pool.clone().pull(sz_indrct_lst) {
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
			(None, None) => Err(VirtqError::BufferNotSpecified),
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
			// Only sending descriptors
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
