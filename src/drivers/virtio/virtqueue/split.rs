//! This module contains Virtio's split virtqueue.
//! See Virito specification v1.1. - 2.6
#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::alloc::{Allocator, Layout};
use core::cell::{RefCell, UnsafeCell};
use core::mem::{size_of, MaybeUninit};
use core::ptr::{self, NonNull};

use virtio_spec::{le16, le32, le64};
use volatile::access::ReadOnly;
use volatile::{map_field, VolatilePtr, VolatileRef};

#[cfg(not(feature = "pci"))]
use super::super::transport::mmio::{ComCfg, NotifCfg, NotifCtrl};
#[cfg(feature = "pci")]
use super::super::transport::pci::{ComCfg, NotifCfg, NotifCtrl};
use super::error::VirtqError;
use super::{
	BuffSpec, BufferToken, BufferType, Bytes, DescrFlags, MemDescr, MemPool, TransferToken, Virtq,
	VirtqPrivate, VqIndex, VqSize,
};
use crate::arch::memory_barrier;
use crate::arch::mm::{paging, VirtAddr};
use crate::mm::device_alloc::DeviceAlloc;

#[repr(C)]
#[derive(Copy, Clone)]
struct Descriptor {
	address: le64,
	len: le32,
	flags: le16,
	next: le16,
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
	flags: le16,
	index: le16,

	// Rust does not allow a field other than the last one to be unsized.
	// Unfortunately, this is not the case with the layout in the specification.
	// For this reason, we merge the last two fields and provide appropriate
	// accessor methods.
	ring_and_event: T,
}

const RING_AND_EVENT_ERROR: &str = "ring_and_event should have at least enough elements for the event. It seems to be allocated incorrectly.";

type AvailRing = GenericRing<[MaybeUninit<le16>]>;

impl AvailRing {
	fn ring_ptr<A: volatile::access::Access>(
		volatile_self: VolatilePtr<'_, Self, A>,
	) -> VolatilePtr<'_, [MaybeUninit<le16>], A> {
		let ring_and_event_ptr = map_field!(volatile_self.ring_and_event);
		ring_and_event_ptr.split_at(ring_and_event_ptr.len()).0
	}

	fn event_ptr<A: volatile::access::Access>(
		volatile_self: VolatilePtr<'_, Self, A>,
	) -> VolatilePtr<'_, MaybeUninit<le16>, A> {
		let ring_and_event_ptr = map_field!(volatile_self.ring_and_event);
		ring_and_event_ptr.index(ring_and_event_ptr.len() - 1)
	}
}

// The elements of the unsized field and the last field are not of the same type.
// For this reason, the field stores raw bytes and we have typed accessors.
type UsedRing = GenericRing<[u8]>;

// Used ring is not supposed to be modified by the driver. Thus, we only have _ref methods (and not _mut methods).
impl UsedRing {
	fn ring_ptr<A: volatile::access::Access>(
		volatile_self: VolatilePtr<'_, Self, A>,
	) -> VolatilePtr<'_, [UsedElem], A> {
		let ring_and_event_ptr = map_field!(volatile_self.ring_and_event);
		let ring_len = (ring_and_event_ptr.len() - size_of::<le16>()) / size_of::<UsedElem>();

		unsafe {
			ring_and_event_ptr.map(|ring_and_event_ptr| {
				NonNull::slice_from_raw_parts(ring_and_event_ptr.cast::<UsedElem>(), ring_len)
			})
		}
	}

	fn event_ptr<A: volatile::access::Access>(
		volatile_self: VolatilePtr<'_, Self, A>,
	) -> VolatilePtr<'_, le16, A> {
		let ring_and_event_ptr = map_field!(volatile_self.ring_and_event);
		let ring_and_event_len = ring_and_event_ptr.len();
		let event_bytes_ptr = ring_and_event_ptr
			.split_at(ring_and_event_len - size_of::<le16>())
			.1;

		unsafe { event_bytes_ptr.map(|event_bytes| event_bytes.cast::<le16>()) }
	}
}

#[repr(C)]
#[derive(Copy, Clone)]
struct UsedElem {
	id: le32,
	len: le32,
}

struct DescrRing {
	read_idx: u16,
	token_ring: Box<[Option<Box<TransferToken>>]>,

	/// Descriptor Tables
	///
	/// # Safety
	///
	/// These tables may only be accessed via volatile operations.
	/// See the corresponding method for a safe wrapper.
	descr_table_cell: Box<UnsafeCell<[MaybeUninit<Descriptor>]>, DeviceAlloc>,
	avail_ring_cell: Box<UnsafeCell<AvailRing>, DeviceAlloc>,
	used_ring_cell: Box<UnsafeCell<UsedRing>, DeviceAlloc>,
}

impl DescrRing {
	fn descr_table_ref(&mut self) -> VolatileRef<'_, [MaybeUninit<Descriptor>]> {
		unsafe { VolatileRef::new(NonNull::new(self.descr_table_cell.get_mut()).unwrap()) }
	}
	fn avail_ring_ref(&mut self) -> VolatileRef<'_, AvailRing> {
		unsafe { VolatileRef::new(NonNull::new(self.avail_ring_cell.get_mut()).unwrap()) }
	}
	fn used_ring_ref(&self) -> VolatileRef<'_, UsedRing, ReadOnly> {
		unsafe { VolatileRef::new_read_only(NonNull::new(self.used_ring_cell.get()).unwrap()) }
	}

	fn push(&mut self, tkn: TransferToken) -> (u16, u8) {
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

			self.descr_table_ref()
				.as_mut_ptr()
				.index(write_indx)
				.write(MaybeUninit::new(descriptor));

			desc_cnt += 1;
			len -= 1;
		}

		self.token_ring[index] = Some(Box::new(tkn));

		let len = self.token_ring.len();
		let mut avail_ring_ref = self.avail_ring_ref();
		let avail_ring = avail_ring_ref.as_mut_ptr();
		let idx = map_field!(avail_ring.index).read().to_ne();
		AvailRing::ring_ptr(avail_ring)
			.index(idx as usize % len)
			.write(MaybeUninit::new((index as u16).into()));

		memory_barrier();
		map_field!(avail_ring.index).update(|val| (val.to_ne().wrapping_add(1)).into());

		(0, 0)
	}

	fn poll(&mut self) {
		// We cannot use a simple while loop here because Rust cannot tell that [Self::used_ring_ref],
		// [Self::read_idx] and [Self::token_ring] access separate fields of `self`. For this reason we
		// need to move [Self::used_ring_ref] lines into a separate scope.
		loop {
			let used_elem;
			{
				let used_ring_ref = self.used_ring_ref();
				let used_ring = used_ring_ref.as_ptr();
				if self.read_idx == map_field!(used_ring.index).read().to_ne() {
					break;
				} else {
					let cur_ring_index = self.read_idx as usize % self.token_ring.len();
					used_elem = UsedRing::ring_ptr(used_ring).index(cur_ring_index).read();
				}
			}

			let mut tkn = self.token_ring[used_elem.id.to_ne() as usize]
				.take()
				.expect(
					"The buff_id is incorrect or the reference to the TransferToken was misplaced.",
				);

			if tkn.buff_tkn.as_ref().unwrap().recv_buff.as_ref().is_some() {
				tkn.buff_tkn
					.as_mut()
					.unwrap()
					.restr_size(None, Some(used_elem.len.to_ne() as usize))
					.unwrap();
			}
			if let Some(queue) = tkn.await_queue.take() {
				queue.try_send(Box::new(tkn.buff_tkn.unwrap())).unwrap()
			}
			memory_barrier();
			self.read_idx = self.read_idx.wrapping_add(1);
		}
	}

	fn drv_enable_notif(&mut self) {
		let mut avail_ring_ref = self.avail_ring_ref();
		let avail_ring = avail_ring_ref.as_mut_ptr();
		map_field!(avail_ring.flags).write(0.into());
	}

	fn drv_disable_notif(&mut self) {
		let mut avail_ring_ref = self.avail_ring_ref();
		let avail_ring = avail_ring_ref.as_mut_ptr();
		map_field!(avail_ring.flags).write(1.into());
	}

	fn dev_is_notif(&self) -> bool {
		let used_ring_ref = self.used_ring_ref();
		let used_ring = used_ring_ref.as_ptr();
		map_field!(used_ring.flags).read().to_ne() & 1 == 0
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
		_await_queue: super::BufferTokenSender,
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
		// Get a handler to the queues configuration area.
		let mut vq_handler = match com_cfg.select_vq(index.into()) {
			Some(handler) => handler,
			None => return Err(VirtqError::QueueNotExisting(index.into())),
		};

		let size = vq_handler.set_vq_size(size.0);
		const ALLOCATOR: DeviceAlloc = DeviceAlloc;

		let descr_table_cell = unsafe {
			core::mem::transmute::<
				Box<[MaybeUninit<Descriptor>], DeviceAlloc>,
				Box<UnsafeCell<[MaybeUninit<Descriptor>]>, DeviceAlloc>,
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
			let ring_and_event_layout = Layout::array::<UsedElem>(size.into())
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

			descr_table_cell,
			avail_ring_cell,
			used_ring_cell,
		};

		let mut notif_ctrl = NotifCtrl::new(notif_cfg.notification_location(&mut vq_handler));

		if features.contains(virtio_spec::F::NOTIFICATION_DATA) {
			notif_ctrl.enable_notif_data();
		}

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

		let ctrl_desc = match self.mem_pool.clone().pull(sz_indrct_lst) {
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
