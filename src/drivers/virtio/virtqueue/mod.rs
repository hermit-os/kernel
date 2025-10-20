//! This module contains Virtio's virtqueue.
//!
//! The virtqueue is available in two forms.
//! [split::SplitVq] and [packed::PackedVq].
//! Both queues are wrapped inside an enum [Virtq] in
//! order to provide an unified interface.
//!
//! Drivers who need a more fine grained access to the specific queues must
//! use the respective virtqueue structs directly.
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

/// A u16 newtype. If instantiated via ``VqIndex::from(T)``, the newtype is ensured to be
/// smaller-equal to `min(u16::MAX , T::MAX)`.
///
/// Currently implements `From<u16>` and `From<u32>`.
#[derive(Copy, Clone, Debug, PartialOrd, PartialEq, Eq)]
pub struct VqIndex(u16);

impl From<u16> for VqIndex {
	fn from(val: u16) -> Self {
		VqIndex(val)
	}
}

impl From<VqIndex> for u16 {
	fn from(i: VqIndex) -> Self {
		i.0
	}
}

impl From<u32> for VqIndex {
	fn from(val: u32) -> Self {
		if val > u32::from(u16::MAX) {
			VqIndex(u16::MAX)
		} else {
			VqIndex(val as u16)
		}
	}
}

/// A u16 newtype. If instantiated via ``VqSize::from(T)``, the newtype is ensured to be
/// smaller-equal to `min(u16::MAX , T::MAX)`.
///
/// Currently implements `From<u16>` and `From<u32>`.
#[derive(Copy, Clone, Debug, PartialOrd, PartialEq, Eq)]
pub struct VqSize(u16);

impl From<u16> for VqSize {
	fn from(val: u16) -> Self {
		VqSize(val)
	}
}

impl From<u32> for VqSize {
	fn from(val: u32) -> Self {
		if val > u32::from(u16::MAX) {
			VqSize(u16::MAX)
		} else {
			VqSize(val as u16)
		}
	}
}

impl From<VqSize> for u16 {
	fn from(val: VqSize) -> Self {
		val.0
	}
}

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

	/// Check if there are no more descriptors left in the queue.
	fn is_empty(&self) -> bool;

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
	fn size(&self) -> VqSize;

	// Returns the index (ID) of a Virtqueue.
	fn index(&self) -> VqIndex;

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
		let addr = table.as_ptr().expose_provenance();
		Self::Descriptor::incomplete_desc(
			u64::try_from(addr).unwrap().into(),
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
					let addr = mem_descr.addr().expose_provenance();
					Self::Descriptor::incomplete_desc(
						u64::try_from(addr).unwrap().into(),
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

	pub fn addr(&self) -> *const u8 {
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
	pub fn pop_front_downcast<T>(&mut self) -> Option<Box<T, DeviceAlloc>>
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
				Err(sized) => {
					// Unlikely and wrong usage, we should not optimize for this case
					self.elems.insert(0, BufferElem::Sized(sized));
					None
				}
			}
		} else {
			// Unlikely and wrong usage, we should not optimize for this case
			self.elems.insert(0, elem);
			None
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
			// Unlikely and wrong usage, we should not optimize for this case
			self.elems.insert(0, elem);
			None
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

/// A newtype for descriptor ids, for better readability.
#[derive(Clone, Copy)]
struct MemDescrId(pub u16);

/// MemPool allows to easily control, request and provide memory for Virtqueues.
///
/// The struct is initialized with a limit of free running "tracked"
/// memory descriptor ids. As Virtqueus do only allow a limited amount of descriptors in their queue,
/// the independent queues, can control the number of descriptors by this.
struct MemPool {
	pool: Vec<MemDescrId>,
	limit: u16,
}

impl MemPool {
	/// Returns a given id to the id pool
	fn ret_id(&mut self, id: MemDescrId) {
		self.pool.push(id);
	}

	/// Returns a new instance, with a pool of the specified size.
	fn new(size: u16) -> MemPool {
		MemPool {
			pool: (0..size).map(MemDescrId).collect(),
			limit: size,
		}
	}

	fn all_used(&self) -> bool {
		self.pool.len() == 0
	}

	fn all_available(&self) -> bool {
		self.pool.len() == self.limit as usize
	}
}

/// Virtqeueus error module.
///
/// This module unifies errors provided to useres of a virtqueue, independent of the underlying
/// virtqueue implementation, realized via the different enum variants.
pub mod error {
	use crate::io;

	#[derive(Debug)]
	// Internal Error Handling for Buffers
	pub enum BufferError {
		WriteToLarge,
		ToManyWrites,
	}

	// External Error Handling for users of the virtqueue.
	pub enum VirtqError {
		General,
		/// Call to create a BufferToken or TransferToken without
		/// any buffers to be inserted
		BufferNotSpecified,
		/// Selected queue does not exist or
		/// is not known to the device and hence can not be used
		QueueNotExisting(u16),
		/// Signals, that the queue does not have any free descriptors
		/// left.
		/// Typically this means, that the driver either has to provide
		/// "unsend" `TransferToken` to the queue (see Docs for details)
		/// or the device needs to process available descriptors in the queue.
		NoDescrAvail,
		/// Indicates that a Bytes::new() call failed or generally that a buffer is to large to
		/// be transferred as one. The Maximum size is u32::MAX. This also is the maximum for indirect
		/// descriptors (both the one placed in the queue, as also the ones the indirect descriptor is
		/// referring to).
		BufferToLarge,
		QueueSizeNotAllowed(u16),
		FeatureNotSupported(virtio::F),
		AllocationError,
		IncompleteWrite,
		NoNewUsed,
	}

	impl core::fmt::Debug for VirtqError {
		fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
			match self {
				VirtqError::General => write!(f, "Virtq failure due to unknown reasons!"),
				VirtqError::BufferNotSpecified => {
					write!(f, "Virtq detected creation of Token, without a BuffSpec")
				}
				VirtqError::QueueNotExisting(_) => {
					write!(f, "Virtq does not exist and can not be used!")
				}
				VirtqError::NoDescrAvail => write!(f, "Virtqs memory pool is exhausted!"),
				VirtqError::BufferToLarge => {
					write!(f, "Buffer to large for queue! u32::MAX exceeded.")
				}
				VirtqError::QueueSizeNotAllowed(_) => {
					write!(f, "The requested queue size is not valid.")
				}
				VirtqError::FeatureNotSupported(_) => {
					write!(f, "An unsupported feature was requested from the queue.")
				}
				VirtqError::AllocationError => write!(
					f,
					"An error was encountered during the allocation of the queue structures."
				),
				VirtqError::IncompleteWrite => {
					write!(f, "A sized object was partially initialized.")
				}
				VirtqError::NoNewUsed => {
					write!(f, "The queue does not contain any new used buffers.")
				}
			}
		}
	}

	impl core::convert::From<VirtqError> for io::Error {
		fn from(_: VirtqError) -> Self {
			io::Error::EIO
		}
	}
}
