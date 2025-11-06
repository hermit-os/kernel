//! Virtqueue infrastructure.
//!
//! [`Virtq`] provides a unified interface for handling either
//! split virtqueues ([`split`]) or packed virtqueues ([`packed`]) transparently.
//!
//! For details on virtqueues, see [Virtqueues].
//!
//! [Virtqueues]: https://docs.oasis-open.org/virtio/virtio/v1.2/cs01/virtio-v1.2-cs01.html#x1-270006

#![allow(dead_code)]

pub mod packed;
pub mod split;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::any::Any;
use core::mem::MaybeUninit;
use core::{mem, ptr};

use enum_dispatch::enum_dispatch;
use smallvec::SmallVec;
use virtio::{le32, le64, pvirtq, virtq};

use self::error::VirtqError;
use crate::drivers::virtio::virtqueue::packed::PackedVq;
use crate::drivers::virtio::virtqueue::split::SplitVq;
use crate::mm::device_alloc::DeviceAlloc;

// Public interface of Virtq

/// The Virtq trait unifies access to the two different Virtqueue types
/// [packed::PackedVq] and [split::SplitVq].
///
/// The trait provides a common interface for both types. Which in some case
/// might not provide the complete feature set of each queue. Drivers who
/// do need these features should refrain from providing support for both
/// Virtqueue types and use the structs directly instead.
#[enum_dispatch]
pub trait Virtq: Send {
	/// The `notif` parameter indicates if the driver wants to have a notification for this specific
	/// transfer. This is only for performance optimization. As it is NOT ensured, that the device sees the
	/// updated notification flags before finishing transfers!
	fn dispatch(
		&mut self,
		tkn: AvailBufferToken,
		notif: bool,
		buffer_type: BufferType,
	) -> Result<(), VirtqError>;

	/// Dispatches the provided TransferToken to the respective queue and does
	/// return when, the queue finished the transfer.
	///
	/// The returned [UsedBufferToken] can be copied from
	/// or return the underlying buffers.
	///
	/// **INFO:**
	/// Currently this function is constantly polling the queue while keeping the notifications disabled.
	/// Upon finish notifications are enabled again.
	fn dispatch_blocking(
		&mut self,
		tkn: AvailBufferToken,
		buffer_type: BufferType,
	) -> Result<UsedBufferToken, VirtqError> {
		self.dispatch(tkn, false, buffer_type)?;

		self.disable_notifs();

		let result: UsedBufferToken;
		// Keep Spinning until the receive queue is filled
		loop {
			// TODO: normally, we should check if the used buffer in question is the one
			// we just made available. However, this shouldn't be a problem as the queue this
			// function is called on makes use of this blocking dispatch function exclusively
			// and thus dispatches cannot be interleaved.
			if let Ok(buffer_tkn) = self.try_recv() {
				result = buffer_tkn;
				break;
			}
		}

		self.enable_notifs();

		Ok(result)
	}

	/// Enables interrupts for this virtqueue upon receiving a transfer
	fn enable_notifs(&mut self);

	/// Disables interrupts for this virtqueue upon receiving a transfer
	fn disable_notifs(&mut self);

	/// Checks if new used descriptors have been written by the device.
	/// This activates the queue and polls the descriptor ring of the queue.
	fn try_recv(&mut self) -> Result<UsedBufferToken, VirtqError>;

	/// Dispatches a batch of [AvailBufferToken]s. The buffers are provided to the queue in
	/// sequence. After the last buffer has been written, the queue marks the first buffer as available and triggers
	/// a device notification if wanted by the device.
	///
	/// The `notif` parameter indicates if the driver wants to have a notification for this specific
	/// transfer. This is only for performance optimization. As it is NOT ensured, that the device sees the
	/// updated notification flags before finishing transfers!
	fn dispatch_batch(
		&mut self,
		tkns: Vec<(AvailBufferToken, BufferType)>,
		notif: bool,
	) -> Result<(), VirtqError>;

	/// Dispatches a batch of [AvailBufferToken]s. The tokens will be placed in to the `await_queue`
	/// upon finish.
	///
	/// The `notif` parameter indicates if the driver wants to have a notification for this specific
	/// transfer. This is only for performance optimization. As it is NOT ensured, that the device sees the
	/// updated notification flags before finishing transfers!
	///
	/// The buffers are provided to the queue in
	/// sequence. After the last buffer has been written, the queue marks the first buffer as available and triggers
	/// a device notification if wanted by the device.
	///
	/// Tokens to get a reference to the provided await_queue, where they will be placed upon finish.
	fn dispatch_batch_await(
		&mut self,
		tkns: Vec<(AvailBufferToken, BufferType)>,
		notif: bool,
	) -> Result<(), VirtqError>;

	/// Returns the size of a Virtqueue. This represents the overall size and not the capacity the
	/// queue currently has for new descriptors.
	fn size(&self) -> u16;

	// Returns the index (ID) of a Virtqueue.
	fn index(&self) -> u16;

	fn has_used_buffers(&self) -> bool;
}

/// These methods are an implementation detail and are meant only for consumption by the default method
/// implementations in [Virtq].
trait VirtqPrivate {
	type Descriptor: VirtqDescriptor;

	fn create_indirect_ctrl(
		buffer_tkn: &AvailBufferToken,
	) -> Result<Box<[Self::Descriptor]>, VirtqError>;

	fn indirect_desc(table: &[Self::Descriptor]) -> Self::Descriptor {
		Self::Descriptor::incomplete_desc(
			DeviceAlloc
				.phys_addr_from(table.as_ptr().cast_mut())
				.as_u64()
				.into(),
			(mem::size_of_val(table) as u32).into(),
			virtq::DescF::INDIRECT,
		)
	}

	/// Consumes the [AvailBufferToken] and returns a [TransferToken], that can be used to actually start the transfer.
	///
	/// After this call, the buffers are no longer writable.
	fn transfer_token_from_buffer_token(
		buff_tkn: AvailBufferToken,
		buffer_type: BufferType,
	) -> TransferToken<Self::Descriptor> {
		let ctrl_desc = match buffer_type {
			BufferType::Direct => None,
			BufferType::Indirect => Some(Self::create_indirect_ctrl(&buff_tkn).unwrap()),
		};

		TransferToken {
			buff_tkn,
			ctrl_desc,
		}
	}

	// The descriptors returned by the iterator will be incomplete, as they do not
	// have all the information necessary.
	fn descriptor_iter(
		buffer_tkn: &AvailBufferToken,
	) -> Result<impl DoubleEndedIterator<Item = Self::Descriptor>, VirtqError> {
		let send_desc_iter = buffer_tkn
			.send_buff
			.iter()
			.map(|elem| (elem, elem.len(), virtq::DescF::empty()));
		let recv_desc_iter = buffer_tkn
			.recv_buff
			.iter()
			.map(|elem| (elem, elem.capacity(), virtq::DescF::WRITE));
		let mut all_desc_iter =
			send_desc_iter
				.chain(recv_desc_iter)
				.map(|(mem_descr, len, incomplete_flags)| {
					Self::Descriptor::incomplete_desc(
						DeviceAlloc
							.phys_addr_from(mem_descr.as_ptr().cast_mut())
							.as_u64()
							.into(),
						len.into(),
						incomplete_flags | virtq::DescF::NEXT,
					)
				});

		let mut last_desc = all_desc_iter
			.next_back()
			.ok_or(VirtqError::BufferNotSpecified)?;
		*last_desc.flags_mut() -= virtq::DescF::NEXT;

		Ok(all_desc_iter.chain([last_desc]))
	}
}

#[enum_dispatch(Virtq)]
pub(crate) enum VirtQueue {
	Split(SplitVq),
	Packed(PackedVq),
}

trait VirtqDescriptor {
	fn flags_mut(&mut self) -> &mut virtq::DescF;

	fn incomplete_desc(addr: virtio::le64, len: virtio::le32, flags: virtq::DescF) -> Self;
}

impl VirtqDescriptor for virtq::Desc {
	fn flags_mut(&mut self) -> &mut virtq::DescF {
		&mut self.flags
	}

	fn incomplete_desc(addr: le64, len: le32, flags: virtq::DescF) -> Self {
		Self {
			addr,
			len,
			flags,
			next: 0.into(),
		}
	}
}

impl VirtqDescriptor for pvirtq::Desc {
	fn flags_mut(&mut self) -> &mut virtq::DescF {
		&mut self.flags
	}

	fn incomplete_desc(addr: le64, len: le32, flags: virtq::DescF) -> Self {
		Self {
			addr,
			len,
			flags,
			id: 0.into(),
		}
	}
}

/// The struct represents buffers which are ready to be send via the
/// virtqueue. Buffers can no longer be written or retrieved.
pub struct TransferToken<Descriptor> {
	/// Must be some in order to prevent drop
	/// upon reuse.
	buff_tkn: AvailBufferToken,
	// Contains the [MemDescr] for the indirect table if the transfer is indirect.
	ctrl_desc: Option<Box<[Descriptor]>>,
}

/// Public Interface for TransferToken
impl<Descriptor> TransferToken<Descriptor> {
	/// Returns the number of descritprors that will be placed in the queue.
	/// This number can differ from the `BufferToken.num_descr()` function value
	/// as indirect buffers only consume one descriptor in the queue, but can have
	/// more descriptors that are accessible via the descriptor in the queue.
	fn num_consuming_descr(&self) -> u16 {
		if self.ctrl_desc.is_some() {
			1
		} else {
			self.buff_tkn.num_descr()
		}
	}
}

#[derive(Debug)]
pub enum BufferElem {
	Sized(Box<dyn Any + Send, DeviceAlloc>),
	Vector(Vec<u8, DeviceAlloc>),
}

impl BufferElem {
	// Returns the initialized length of the element. Assumes [Self::Sized] to
	// be initialized, since the type of the object is erased and we cannot
	// detect if the content is actually a [MaybeUninit]. However, this function
	// should be only relevant for read buffer elements, which should not be uninit.
	// If the element belongs to a write buffer, it is likely that [Self::capacity]
	// is more appropriate.
	pub fn len(&self) -> u32 {
		match self {
			BufferElem::Sized(sized) => mem::size_of_val(sized.as_ref()),
			BufferElem::Vector(vec) => vec.len(),
		}
		.try_into()
		.unwrap()
	}

	pub fn capacity(&self) -> u32 {
		match self {
			BufferElem::Sized(sized) => mem::size_of_val(sized.as_ref()),
			BufferElem::Vector(vec) => vec.capacity(),
		}
		.try_into()
		.unwrap()
	}

	pub fn as_ptr(&self) -> *const u8 {
		match self {
			BufferElem::Sized(sized) => ptr::from_ref(sized.as_ref()).cast::<u8>(),
			BufferElem::Vector(vec) => vec.as_ptr(),
		}
	}
}

/// The struct represents buffers which are ready to be written or to be send.
///
/// BufferTokens can be written in two ways:
/// * in one step via `BufferToken.write()
///   * consumes BufferToken and returns a TransferToken
/// * sequentially via `BufferToken.write_seq()
///
/// # Structure of the Token
/// The token can potentially hold both a *send* and a *recv* buffer, but MUST hold
/// one.
/// The *send* buffer is the data the device will read during a transfer, the *recv* buffer
/// is the data the device will write to during a transfer.
///
/// # What are Buffers
/// A buffer represents multiple chunks of memory. Where each chunk can be of different size.
/// The chunks are named descriptors in the following.
///
/// **For Example:**
/// A buffer could consist of 3 descriptors:
/// 1. First descriptor of 30 bytes
/// 2. Second descriptor of 10 bytes
/// 3. Third descriptor of 100 bytes
///
/// Each of these descriptors consumes one "element" of the
/// respective virtqueue.
/// The maximum number of descriptors per buffer is bounded by the size of the virtqueue.
pub struct AvailBufferToken {
	pub(crate) send_buff: SmallVec<[BufferElem; 2]>,
	pub(crate) recv_buff: SmallVec<[BufferElem; 2]>,
}

pub(crate) struct UsedDeviceWritableBuffer {
	elems: SmallVec<[BufferElem; 2]>,
	remaining_written_len: u32,
}

impl UsedDeviceWritableBuffer {
	/// # Safety
	/// The type `T` of the [`MaybeUninit<T>`] that was provided for the [BufferElem] must have
	/// been what the device will write at that portion of the buffer. This may not be the case if
	/// the descriptor is used by the device for another type (e.g. in the case of merged buffers
	/// in the network driver), as the objects are assumed to be initialized by this function and
	/// it's undefined behavior to call [MaybeUninit::assume_init] when an object of the correct type
	/// is not initialized.
	pub unsafe fn pop_front_downcast<T>(&mut self) -> Option<Box<T, DeviceAlloc>>
	where
		T: Any,
	{
		if self.remaining_written_len < u32::try_from(size_of::<T>()).unwrap() {
			return None;
		}

		// May panic, but we have written data remaining so there should always be an item
		let elem = if self.elems.len() <= 2 {
			self.elems.swap_remove(0)
		} else {
			self.elems.remove(0)
		};

		if let BufferElem::Sized(sized) = elem {
			match sized.downcast::<MaybeUninit<T>>() {
				Ok(cast) => {
					self.remaining_written_len -= u32::try_from(size_of::<T>()).unwrap();
					Some(unsafe { cast.assume_init() })
				}
				Err(_) => {
					panic!("Attempted to downcast element to wrong type");
				}
			}
		} else {
			panic!("Attempted to pop elements in order different from insertion");
		}
	}

	pub fn pop_front_vec(&mut self) -> Option<Vec<u8, DeviceAlloc>> {
		if self.elems.is_empty() {
			return None;
		}

		let elem = if self.elems.len() <= 2 {
			self.elems.swap_remove(0)
		} else {
			self.elems.remove(0)
		};

		if let BufferElem::Vector(mut vector) = elem {
			let new_len = u32::min(
				vector.capacity().try_into().unwrap(),
				self.remaining_written_len,
			);
			self.remaining_written_len -= new_len;
			unsafe { vector.set_len(new_len.try_into().unwrap()) };
			Some(vector)
		} else {
			panic!("Attempted to pop elements in order different from insertion");
		}
	}

	/// It is possible for devices to use descriptors for a type other than what they were meant.
	/// (e.g. for a portion of the received frame in the network driver when [virtio::net::F::MRG_RXBUF] is negotiated).
	/// In that case, we hand over the popped box directly with the used length.
	///
	/// We may not return a vector as its layout would be different and deallocation would not be correct
	/// (see the information for [Box::into_non_null_with_allocator]).
	pub fn pop_front_raw(&mut self) -> Option<(Box<dyn Any + Send, DeviceAlloc>, usize)> {
		let elem = if self.elems.len() <= 2 {
			self.elems.swap_remove(0)
		} else {
			self.elems.remove(0)
		};

		if let BufferElem::Sized(sized) = elem {
			let capacity = u32::try_from(size_of_val(sized.as_ref())).unwrap();
			let len = u32::min(capacity, self.remaining_written_len);
			self.remaining_written_len -= len;
			Some((sized, len.try_into().unwrap()))
		} else {
			panic!("This function is meant for the Sized variant of the BufferElem enum.");
		}
	}
}

pub(crate) struct UsedBufferToken {
	pub send_buff: SmallVec<[BufferElem; 2]>,
	pub used_recv_buff: UsedDeviceWritableBuffer,
}

impl UsedBufferToken {
	fn from_avail_buffer_token(tkn: AvailBufferToken, written_len: u32) -> Self {
		Self {
			send_buff: tkn.send_buff,
			used_recv_buff: UsedDeviceWritableBuffer {
				elems: tkn.recv_buff,
				remaining_written_len: written_len,
			},
		}
	}
}

// Private interface of BufferToken
impl AvailBufferToken {
	/// Returns the overall number of descriptors.
	fn num_descr(&self) -> u16 {
		u16::try_from(self.send_buff.len() + self.recv_buff.len()).unwrap()
	}
}

// Public interface of BufferToken
impl AvailBufferToken {
	/// **Parameters**
	/// * send: The slices that will make up the elements of the driver-writable buffer.
	/// * recv: The slices that will make up the elements of the device-writable buffer.
	///
	/// **Reasons for Failure:**
	/// * Both `send` and `recv` are empty, which is not allowed by Virtio.
	///
	/// * If one wants to have a structure in the style of:
	/// ```
	/// struct send_recv_struct {
	///     // send_part: ...
	///     // recv_part: ...
	/// }
	/// ```
	/// they must split the structure after the send part and provide the respective part via the send argument and the respective other
	/// part via the recv argument.
	pub fn new(
		send_buff: SmallVec<[BufferElem; 2]>,
		recv_buff: SmallVec<[BufferElem; 2]>,
	) -> Result<Self, VirtqError> {
		if send_buff.is_empty() && recv_buff.is_empty() {
			return Err(VirtqError::BufferNotSpecified);
		}

		Ok(Self {
			send_buff,
			recv_buff,
		})
	}
}

pub enum BufferType {
	/// As many descriptors get consumed in the descriptor table as the sum of the numbers of slices in [AvailBufferToken::send_buff] and [AvailBufferToken::recv_buff].
	Direct,
	/// Results in one descriptor in the queue, hence consumes one element in the main descriptor table. The queue will merge the send and recv buffers as follows:
	/// ```text
	/// //+++++++++++++++++++++++
	/// //+        Queue        +
	/// //+++++++++++++++++++++++
	/// //+ Indirect descriptor + -> refers to a descriptor list in the form of ->  ++++++++++++++++++++++++++
	/// //+         ...         +                                                   +  Descriptors for send  +
	/// //+++++++++++++++++++++++                                                   +  Descriptors for recv  +
	/// //                                                                          ++++++++++++++++++++++++++
	/// ```
	/// As a result indirect descriptors result in a single descriptor consumption in the actual queue.
	Indirect,
}

/// MemPool allows to easily control, request and provide memory for Virtqueues.
///
/// The struct is initialized with a limit of free running "tracked"
/// memory descriptor ids. As Virtqueus do only allow a limited amount of descriptors in their queue,
/// the independent queues, can control the number of descriptors by this.
struct MemPool {
	pool: Vec<u16>,
	limit: u16,
}

impl MemPool {
	/// Returns a given id to the id pool
	fn ret_id(&mut self, id: u16) {
		self.pool.push(id);
	}

	/// Returns a new instance, with a pool of the specified size.
	fn new(size: u16) -> MemPool {
		MemPool {
			pool: (0..size).collect(),
			limit: size,
		}
	}
}

/// Virtqeueus error module.
///
/// This module unifies errors provided to useres of a virtqueue, independent of the underlying
/// virtqueue implementation, realized via the different enum variants.
pub mod error {
	use thiserror::Error;

	use crate::errno::Errno;

	// External Error Handling for users of the virtqueue.
	#[derive(Error, Debug)]
	pub enum VirtqError {
		#[error("Virtq failure due to unknown reasons!")]
		General,

		/// Call to create a BufferToken or TransferToken without
		/// any buffers to be inserted
		#[error("Virtq detected creation of Token, without a BuffSpec")]
		BufferNotSpecified,

		/// Selected queue does not exist or
		/// is not known to the device and hence can not be used
		#[error("Virtq does not exist and can not be used!")]
		QueueNotExisting(u16),

		/// Signals, that the queue does not have any free descriptors
		/// left.
		/// Typically this means, that the driver either has to provide
		/// "unsend" `TransferToken` to the queue (see Docs for details)
		/// or the device needs to process available descriptors in the queue.
		#[error("Virtqs memory pool is exhausted!")]
		NoDescrAvail,

		/// Indicates that a Bytes::new() call failed or generally that a buffer is to large to
		/// be transferred as one. The Maximum size is u32::MAX. This also is the maximum for indirect
		/// descriptors (both the one placed in the queue, as also the ones the indirect descriptor is
		/// referring to).
		#[error("Buffer to large for queue! u32::MAX exceeded.")]
		BufferToLarge,

		#[error("The requested queue size is not valid.")]
		QueueSizeNotAllowed(u16),

		#[error("An unsupported feature was requested from the queue.")]
		FeatureNotSupported(virtio::F),

		#[error("An error was encountered during the allocation of the queue structures.")]
		AllocationError,

		#[error("A sized object was partially initialized.")]
		IncompleteWrite,

		#[error("The queue does not contain any new used buffers.")]
		NoNewUsed,
	}

	impl core::convert::From<VirtqError> for Errno {
		fn from(_: VirtqError) -> Self {
			Errno::Io
		}
	}
}
