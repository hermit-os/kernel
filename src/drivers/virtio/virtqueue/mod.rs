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
#![allow(clippy::type_complexity)]

pub mod packed;
pub mod split;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::any::Any;
use core::cell::RefCell;
use core::{mem, ptr};

use async_channel::TryRecvError;

use self::error::VirtqError;
#[cfg(not(feature = "pci"))]
use super::transport::mmio::{ComCfg, NotifCfg};
#[cfg(feature = "pci")]
use super::transport::pci::{ComCfg, NotifCfg};
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
		if val > u16::MAX as u32 {
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
		if val > u16::MAX as u32 {
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

type BufferTokenSender = async_channel::Sender<BufferToken>;

// Public interface of Virtq

/// The Virtq trait unifies access to the two different Virtqueue types
/// [packed::PackedVq] and [split::SplitVq].
///
/// The trait provides a common interface for both types. Which in some case
/// might not provide the complete feature set of each queue. Drivers who
/// do need these features should refrain from providing support for both
/// Virtqueue types and use the structs directly instead.
#[allow(private_bounds)]
pub trait Virtq {
	/// The `notif` parameter indicates if the driver wants to have a notification for this specific
	/// transfer. This is only for performance optimization. As it is NOT ensured, that the device sees the
	/// updated notification flags before finishing transfers!
	fn dispatch(
		&self,
		tkn: BufferToken,
		sender: Option<BufferTokenSender>,
		notif: bool,
		buffer_type: BufferType,
	) -> Result<(), VirtqError>;

	/// Dispatches the provided TransferToken to the respective queue and does
	/// return when, the queue finished the transfer.
	///
	/// The returned [BufferToken] can be reused, copied from
	/// or return the underlying buffers.
	///
	/// **INFO:**
	/// Currently this function is constantly polling the queue while keeping the notifications disabled.
	/// Upon finish notifications are enabled again.
	fn dispatch_blocking(
		&self,
		tkn: BufferToken,
		buffer_type: BufferType,
	) -> Result<BufferToken, VirtqError> {
		let (sender, receiver) = async_channel::bounded(1);
		self.dispatch(tkn, Some(sender), false, buffer_type)?;

		self.disable_notifs();

		let result: BufferToken;
		// Keep Spinning until the receive queue is filled
		loop {
			match receiver.try_recv() {
				Ok(buffer_tkn) => {
					result = buffer_tkn;
					break;
				}
				Err(TryRecvError::Closed) => return Err(VirtqError::General),
				Err(TryRecvError::Empty) => self.poll(),
			}
		}

		self.enable_notifs();

		Ok(result)
	}

	/// Enables interrupts for this virtqueue upon receiving a transfer
	fn enable_notifs(&self);

	/// Disables interrupts for this virtqueue upon receiving a transfer
	fn disable_notifs(&self);

	/// Checks if new used descriptors have been written by the device.
	/// This activates the queue and polls the descriptor ring of the queue.
	///
	/// * `TransferTokens` which hold an `await_queue` will be placed into
	///   these queues.
	fn poll(&self);

	/// Dispatches a batch of [BufferToken]s. The buffers are provided to the queue in
	/// sequence. After the last buffer has been written, the queue marks the first buffer as available and triggers
	/// a device notification if wanted by the device.
	///
	/// The `notif` parameter indicates if the driver wants to have a notification for this specific
	/// transfer. This is only for performance optimization. As it is NOT ensured, that the device sees the
	/// updated notification flags before finishing transfers!
	fn dispatch_batch(
		&self,
		tkns: Vec<(BufferToken, BufferType)>,
		notif: bool,
	) -> Result<(), VirtqError>;

	/// Dispatches a batch of [BufferToken]s. The tokens will be placed in to the `await_queue`
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
		&self,
		tkns: Vec<(BufferToken, BufferType)>,
		await_queue: BufferTokenSender,
		notif: bool,
	) -> Result<(), VirtqError>;

	/// Creates a new Virtq of the specified [VqSize] and the [VqIndex].
	/// The index represents the "ID" of the virtqueue.
	/// Upon creation the virtqueue is "registered" at the device via the `ComCfg` struct.
	///
	/// Be aware, that devices define a maximum number of queues and a maximal size they can handle.
	fn new(
		com_cfg: &mut ComCfg,
		notif_cfg: &NotifCfg,
		size: VqSize,
		index: VqIndex,
		features: virtio::F,
	) -> Result<Self, VirtqError>
	where
		Self: Sized;

	/// Returns the size of a Virtqueue. This represents the overall size and not the capacity the
	/// queue currently has for new descriptors.
	fn size(&self) -> VqSize;

	// Returns the index (ID) of a Virtqueue.
	fn index(&self) -> VqIndex;
}

/// These methods are an implementation detail and are meant only for consumption by the default method
/// implementations in [Virtq].
trait VirtqPrivate {
	type Descriptor;

	fn create_indirect_ctrl(
		&self,
		send: &[BufferElem],
		recv: &[BufferElem],
	) -> Result<Box<[Self::Descriptor]>, VirtqError>;

	/// Consumes the [BufferToken] and returns a [TransferToken], that can be used to actually start the transfer.
	///
	/// After this call, the buffers are no longer writable.
	fn transfer_token_from_buffer_token(
		&self,
		buff_tkn: BufferToken,
		await_queue: Option<BufferTokenSender>,
		buffer_type: BufferType,
	) -> TransferToken<Self::Descriptor> {
		let ctrl_desc = match buffer_type {
			BufferType::Direct => None,
			BufferType::Indirect => Some(
				self.create_indirect_ctrl(&buff_tkn.send_buff, &buff_tkn.recv_buff)
					.unwrap(),
			),
		};

		TransferToken {
			buff_tkn,
			await_queue,
			ctrl_desc,
		}
	}
}

/// The struct represents buffers which are ready to be send via the
/// virtqueue. Buffers can no longer be written or retrieved.
pub struct TransferToken<Descriptor> {
	/// Must be some in order to prevent drop
	/// upon reuse.
	buff_tkn: BufferToken,
	/// Structure which allows to await Transfers
	/// If Some, finished TransferTokens will be placed here
	/// as finished `Transfers`. If None, only the state
	/// of the Token will be changed.
	await_queue: Option<BufferTokenSender>,
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
	Sized(Box<dyn Any, DeviceAlloc>),
	Vector(Vec<u8, DeviceAlloc>),
}

impl BufferElem {
	pub fn downcast<T>(self) -> Result<Box<T, DeviceAlloc>, Self>
	where
		T: Any,
	{
		if let Self::Sized(sized) = self {
			match sized.downcast() {
				Ok(cast) => Ok(cast),
				Err(sized) => Err(Self::Sized(sized)),
			}
		} else {
			Err(self)
		}
	}

	pub fn try_into_vec(self) -> Option<Vec<u8, DeviceAlloc>> {
		if let Self::Vector(vec) = self {
			Some(vec)
		} else {
			None
		}
	}

	// Returns the initialized length of the element. Assumes [Self::Sized] to
	// be initialized, since the type of the object is erased and we cannot
	// detect if the content is actuallly a [MaybeUninit]. However, this function
	// should be only relevant for read buffer elements, which should not be uninit.
	// If the element belongs to a write buffer, it is likely that [Self::capacity]
	// is more appropriate.
	pub fn len(&self) -> u16 {
		match self {
			BufferElem::Sized(sized) => mem::size_of_val(sized.as_ref()),
			BufferElem::Vector(vec) => vec.len(),
		}
		.try_into()
		.unwrap()
	}

	pub fn capacity(&self) -> u16 {
		match self {
			BufferElem::Sized(sized) => mem::size_of_val(sized.as_ref()),
			BufferElem::Vector(vec) => vec.capacity(),
		}
		.try_into()
		.unwrap()
	}

	pub fn addr(&self) -> *const u8 {
		match self {
			BufferElem::Sized(sized) => ptr::from_ref(sized.as_ref()) as *const u8,
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
pub struct BufferToken {
	pub(crate) send_buff: Vec<BufferElem>,
	pub(crate) recv_buff: Vec<BufferElem>,
}

// Private interface of BufferToken
impl BufferToken {
	/// Returns the overall number of descriptors.
	fn num_descr(&self) -> u16 {
		u16::try_from(self.send_buff.len() + self.recv_buff.len()).unwrap()
	}

	/// Updates the len of the byte vectors accessible by the drivers to be consistent with
	/// the amount of data written by the device.
	fn set_device_written_len(&mut self, len: u32) -> Result<(), VirtqError> {
		let mut remaining_len = usize::try_from(len).unwrap();
		for elem in &mut self.recv_buff {
			match elem {
				BufferElem::Sized(sized) => {
					let object_size = mem::size_of_val(sized.as_ref());
					// Partially initialized sized objects are invalid
					if remaining_len < object_size {
						return Err(VirtqError::IncompleteWrite);
					}
					remaining_len -= object_size;
				}
				BufferElem::Vector(vec) => {
					let new_len = vec.capacity().min(remaining_len);
					unsafe { vec.set_len(new_len) }
					remaining_len -= new_len;
				}
			}
		}
		Ok(())
	}
}

// Public interface of BufferToken
impl BufferToken {
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
	pub fn new(send_buff: Vec<BufferElem>, recv_buff: Vec<BufferElem>) -> Result<Self, VirtqError> {
		if send_buff.is_empty() && recv_buff.is_empty() {
			return Err(VirtqError::BufferNotSpecified);
		}

		Ok(Self {
			recv_buff,
			send_buff,
		})
	}
}

pub enum BufferType {
	/// As many descriptors get consumed in the descriptor table as the sum of the numbers of slices in [BufferToken::send_buff] and [BufferToken::recv_buff].
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
	pool: RefCell<Vec<MemDescrId>>,
	limit: u16,
}

impl MemPool {
	/// Returns a given id to the id pool
	fn ret_id(&self, id: MemDescrId) {
		self.pool.borrow_mut().push(id);
	}

	/// Returns a new instance, with a pool of the specified size.
	fn new(size: u16) -> MemPool {
		MemPool {
			pool: RefCell::new((0..size).map(MemDescrId).collect()),
			limit: size,
		}
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
			}
		}
	}

	impl core::convert::From<VirtqError> for io::Error {
		fn from(_: VirtqError) -> Self {
			io::Error::EIO
		}
	}
}
