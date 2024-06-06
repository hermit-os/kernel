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
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::ops::{BitAnd, Deref, DerefMut};
use core::ptr;

use align_address::Align;
use async_channel::TryRecvError;
use zerocopy::AsBytes;

use self::error::{BufferError, VirtqError};
#[cfg(not(feature = "pci"))]
use super::transport::mmio::{ComCfg, NotifCfg};
#[cfg(feature = "pci")]
use super::transport::pci::{ComCfg, NotifCfg};
use crate::arch::mm::paging::{BasePageSize, PageSize};
use crate::arch::mm::{paging, VirtAddr};

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

/// The General Descriptor struct for both Packed and SplitVq.
#[repr(C, align(16))]
struct Descriptor {
	address: u64,
	len: u32,
	buff_id: u16,
	flags: u16,
}

type BufferTokenSender = async_channel::Sender<Box<BufferToken>>;

// Public interface of Virtq

/// The Virtq trait unifies access to the two different Virtqueue types
/// [packed::PackedVq] and [split::SplitVq].
///
/// The trait provides a common interface for both types. Which in some case
/// might not provide the complete feature set of each queue. Drivers who
/// do need these features should refrain from providing support for both
/// Virtqueue types and use the structs directly instead.
#[allow(private_bounds)]
pub trait Virtq: VirtqPrivate {
	/// Entry function which the TransferTokens can use, when they are dispatching
	/// themselves via their `Rc<Virtq>` reference
	///
	/// The `notif` parameter indicates if the driver wants to have a notification for this specific
	/// transfer. This is only for performance optimization. As it is NOT ensured, that the device sees the
	/// updated notification flags before finishing transfers!
	fn dispatch(&self, tkn: TransferToken, notif: bool);

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

	/// Dispatches a batch of transfer token. The buffers of the respective transfers are provided to the queue in
	/// sequence. After the last buffer has been written, the queue marks the first buffer as available and triggers
	/// a device notification if wanted by the device.
	///
	/// The `notif` parameter indicates if the driver wants to have a notification for this specific
	/// transfer. This is only for performance optimization. As it is NOT ensured, that the device sees the
	/// updated notification flags before finishing transfers!
	fn dispatch_batch(&self, tkns: Vec<TransferToken>, notif: bool);

	/// Dispatches a batch of TransferTokens. The Transfers will be placed in to the `await_queue`
	/// upon finish.
	///
	/// The `notif` parameter indicates if the driver wants to have a notification for this specific
	/// transfer. This is only for performance optimization. As it is NOT ensured, that the device sees the
	/// updated notification flags before finishing transfers!
	///
	/// Dispatches a batch of transfer token. The buffers of the respective transfers are provided to the queue in
	/// sequence. After the last buffer has been written, the queue marks the first buffer as available and triggers
	/// a device notification if wanted by the device.
	///
	/// Tokens to get a reference to the provided await_queue, where they will be placed upon finish.
	fn dispatch_batch_await(
		&self,
		tkns: Vec<TransferToken>,
		await_queue: BufferTokenSender,
		notif: bool,
	);

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
		features: virtio_spec::F,
	) -> Result<Self, VirtqError>
	where
		Self: Sized;

	/// Returns the size of a Virtqueue. This represents the overall size and not the capacity the
	/// queue currently has for new descriptors.
	fn size(&self) -> VqSize;

	// Returns the index (ID) of a Virtqueue.
	fn index(&self) -> VqIndex;

	/// Provides the calley with a TransferToken. Fails upon multiple circumstances.
	///
	/// **INFO:**
	/// * Data behind the respective raw pointers will NOT be deallocated. Under no circumstances.
	/// * Calley is responsible for ensuring the raw pointers will remain valid from start till end of transfer.
	///   * start: call of `fn prep_transfer_from_raw()`
	///   * end: return of the [BufferToken] via [TransferToken::dispatch_blocking] or its push to the [TransferToken::await_queue].
	///   * In case the underlying BufferToken is reused, the raw pointers MUST still be valid all the time
	///     [BufferToken] exists.
	/// * [BufferToken] created from this TransferTokens will ONLY allow to return a copy of the data.
	///   * This is due to the fact, that the `Transfer.ret()` returns a `Box[u8]`, which must own
	///     the array. This would lead to unwanted frees, if not handled carefully
	/// * Drivers must take care of keeping a copy of the respective `*mut T` and `*mut K` for themselves
	///
	/// **Parameters**
	/// * send: `Option<(*mut T, BuffSpec)>`
	///     * None: No send buffers are provided to the device
	///     * Some:
	///         * `T` defines the structure which will be provided to the device
	///         * [BuffSpec] defines how this struct will be presented to the device.
	///           See documentation on `BuffSpec` for details.
	/// * recv: `Option<(*mut K, BuffSpec)>`
	///     * None: No buffers, which are writable for the device are provided to the device.
	///     * Some:
	///         * `K` defines the structure which will be provided to the device
	///         * [BuffSpec] defines how this struct will be presented to the device.
	///           See documentation on `BuffSpec` for details.
	///
	/// **Reasons for Failure:**
	/// * Queue does not have enough descriptors left, to split `T` or `K` into the desired amount of memory chunks.
	/// * Calley mixed `Indirect (Direct::Indirect())` with `Direct(BuffSpec::Single() or BuffSpec::Multiple())` descriptors.
	///
	/// **Details on Usage:**
	/// * `(Single, _ )` or `(_ , Single)` -> Results in one descriptor in the queue, hence Consumes one element.
	/// * `(Multiple, _ )` or `(_ , Multiple)` -> Results in a list of descriptors in the queue. Consumes `Multiple.len()` elements.
	/// * `(Single, Single)` -> Results in a descriptor list of two chained descriptors, hence Consumes two elements in the queue
	/// * `(Single, Multiple)` or `(Multiple, Single)` -> Results in a descripotr list of `1 + Multiple.len(). Consumes equally
	///   many elements in the queue.
	/// * `(Indirect, _ )` or `(_, Indirect)` -> Resulsts in one descriptor in the queue, hence Consumes one element.
	/// * `(Indirect, Indirect)` -> Resulsts in one descriptor in the queue, hence Consumes one element.
	///    * Calley is not allowed to mix `Indirect` and `Direct` descriptors. Furthermore if the calley decides to use `Indirect`
	///      descriptors, the queue will merge the send and recv structure as follows:
	/// ```text
	/// //+++++++++++++++++++++++
	/// //+        Queue        +
	/// //+++++++++++++++++++++++
	/// //+ Indirect descriptor + -> refers to a descriptor list in the form of ->  ++++++++++++++++++++++++++
	/// //+         ...         +                                                   +     Descriptors for T  +
	/// //+++++++++++++++++++++++                                                   +     Descriptors for K  +
	/// //                                                                          ++++++++++++++++++++++++++
	/// ```
	/// As a result indirect descriptors result in a single descriptor consumption in the actual queue.
	///
	/// * If one wants to have a structure in the style of:
	/// ```
	/// struct send_recv_struct {
	///     // send_part: ...
	///     // recv_part: ...
	/// }
	/// ```
	/// Then he must split the structure after the send part and provide the respective part via the send argument and the respective other
	/// part via the recv argument.
	fn prep_transfer_from_raw(
		self: Rc<Self>,
		send: &[&[u8]],
		recv: &[&mut [u8]],
		buffer_type: BufferType,
	) -> Result<TransferToken, VirtqError>;

	/// The implementation of the method requires constraints that are incompatible with a trait object.
	/// Because of this, we constrain it to static objects (via Sized) and call it from the implementation
	/// of [Self::prep_buffer] inside the implementor.
	fn prep_transfer_from_raw_static(
		self: Rc<Self>,
		send: &[&[u8]],
		recv: &[&mut [u8]],
		buffer_type: BufferType,
	) -> Result<TransferToken, VirtqError>
	where
		Self: Sized + 'static,
	{
		if send.is_empty() && recv.is_empty() {
			return Err(VirtqError::BufferNotSpecified);
		}

		let total_send_len = send.iter().map(|slice| slice.len()).sum();
		let total_recv_len = recv.iter().map(|slice| slice.len()).sum();

		let send_buff;
		let recv_buff;
		match buffer_type {
			BufferType::Direct => {
				let send_desc_lst = send
					.iter()
					.map(|slice| self.mem_pool().pull_from_raw(slice))
					.collect::<Result<Vec<_>, VirtqError>>()?;
				send_buff = if !send.is_empty() {
					Some(Buffer::Multiple {
						desc_lst: send_desc_lst.into_boxed_slice(),
						len: total_send_len,
						next_write: 0,
					})
				} else {
					None
				};

				let recv_desc_lst: Vec<_> = recv
					.iter()
					.map(|slice| self.mem_pool().pull_from_raw(slice))
					.collect::<Result<Vec<_>, VirtqError>>()?;
				recv_buff = if !recv.is_empty() {
					Some(Buffer::Multiple {
						desc_lst: recv_desc_lst.into_boxed_slice(),
						len: total_recv_len,
						next_write: 0,
					})
				} else {
					None
				}
			}
			BufferType::Indirect => {
				let send_desc_lst: Vec<_> = send
					.iter()
					.map(|slice| self.mem_pool().pull_from_raw_untracked(slice))
					.collect();
				let recv_desc_lst: Vec<_> = recv
					.iter()
					.map(|slice| self.mem_pool().pull_from_raw_untracked(slice))
					.collect();

				let ctrl_desc = self.create_indirect_ctrl(
					if !send.is_empty() {
						Some(&send_desc_lst)
					} else {
						None
					},
					if !recv.is_empty() {
						Some(&recv_desc_lst)
					} else {
						None
					},
				)?;

				let mut send_ctrl_desc = None;
				let mut recv_ctrl_desc = None;
				match (!send.is_empty(), !recv.is_empty()) {
					(true, false) => {
						send_ctrl_desc = Some(ctrl_desc);
					}
					(false, true) => {
						recv_ctrl_desc = Some(ctrl_desc);
					}
					// Both send and recv have content
					(true, true) => {
						send_ctrl_desc = Some(ctrl_desc.no_dealloc_clone());
						recv_ctrl_desc = Some(ctrl_desc);
					}
					// We checked at the beginning of the function.
					(false, false) => unreachable!(),
				}

				send_buff = if !send.is_empty() {
					Some(Buffer::Indirect {
						desc_lst: send_desc_lst.into_boxed_slice(),
						ctrl_desc: send_ctrl_desc.unwrap(),
						len: total_send_len,
						next_write: 0,
					})
				} else {
					None
				};

				recv_buff = if !recv.is_empty() {
					Some(Buffer::Indirect {
						desc_lst: recv_desc_lst.into_boxed_slice(),
						ctrl_desc: recv_ctrl_desc.unwrap(),
						len: total_recv_len,
						next_write: 0,
					})
				} else {
					None
				};
			}
		};

		Ok(TransferToken {
			buff_tkn: Some(BufferToken {
				recv_buff,
				send_buff,
				vq: self,
				ret_send: false,
				ret_recv: false,
				reusable: false,
			}),
			await_queue: None,
		})
	}

	/// Provides the calley with empty buffers as specified via the `send` and `recv` function parameters, (see [BuffSpec]), in form of
	/// a [BufferToken].
	/// Fails upon multiple circumstances.
	///
	/// **Parameters**
	/// * send: `Option<BuffSpec>`
	///     * None: No send buffers are provided to the device
	///     * Some:
	///         * [BuffSpec] defines the size of the buffer and how the buffer is
	///           Buffer will be structured. See documentation on `BuffSpec` for details.
	/// * recv: `Option<BuffSpec>`
	///     * None: No buffers, which are writable for the device are provided to the device.
	///     * Some:
	///         * [BuffSpec] defines the size of the buffer and how the buffer is
	///           Buffer will be structured. See documentation on `BuffSpec` for details.
	///
	/// **Reasons for Failure:**
	/// * Queue does not have enough descriptors left to create the desired amount of descriptors as indicated by the `BuffSpec`.
	/// * Calley mixed `Indirect (Direct::Indirect())` with `Direct(BuffSpec::Single() or BuffSpec::Multiple())` descriptors.
	/// * Systerm does not have enough memory resources left.
	///
	/// **Details on Usage:**
	/// * `(Single, _ )` or `(_ , Single)` -> Results in one descriptor in the queue, hence Consumes one element.
	/// * `(Multiple, _ )` or `(_ , Multiple)` -> Results in a list of descriptors in the queue. Consumes `Multiple.len()` elements.
	/// * `(Single, Single)` -> Results in a descriptor list of two chained descriptors, hence Consumes two elements in the queue
	/// * `(Single, Multiple)` or `(Multiple, Single)` -> Results in a descripotr list of `1 + Multiple.len(). Consumes equally
	///   many elements in the queue.
	/// * `(Indirect, _ )` or `(_, Indirect)` -> Resulsts in one descriptor in the queue, hence Consumes one element.
	/// * `(Indirect, Indirect)` -> Resulsts in one descriptor in the queue, hence Consumes one element.
	///    * Calley is not allowed to mix `Indirect` and `Direct` descriptors. Furthermore if the calley decides to use `Indirect`
	///      descriptors, the queue will merge the send and recv structure as follows:
	/// ```text
	/// //+++++++++++++++++++++++
	/// //+        Queue        +
	/// //+++++++++++++++++++++++
	/// //+ Indirect descriptor + -> refers to a descriptor list in the form of ->  ++++++++++++++++++++++++++
	/// //+         ...         +                                                   +     Descriptors for T  +
	/// //+++++++++++++++++++++++                                                   +     Descriptors for K  +
	/// //                                                                          ++++++++++++++++++++++++++
	/// ```
	/// As a result indirect descriptors result in a single descriptor consumption in the actual queue.
	fn prep_buffer(
		self: Rc<Self>,
		send: Option<BuffSpec<'_>>,
		recv: Option<BuffSpec<'_>>,
	) -> Result<BufferToken, VirtqError>;

	/// The implementation of the method requires constraints that are incompatible with a trait object.
	/// Because of this, we constrain it to static objects (via Sized) and call it from the implementation
	/// of [Self::prep_buffer] inside the implementor.
	fn prep_buffer_static(
		self: Rc<Self>,
		send: Option<BuffSpec<'_>>,
		recv: Option<BuffSpec<'_>>,
	) -> Result<BufferToken, VirtqError>
	where
		Self: Sized + 'static,
	{
		match (send, recv) {
			// No buffers specified
			(None, None) => Err(VirtqError::BufferNotSpecified),
			// Send buffer specified, No recv buffer
			(Some(spec), None) => {
				match spec {
					BuffSpec::Single(size) => match self.mem_pool().pull(size) {
						Ok(desc) => {
							let buffer = Buffer::Single {
								desc_lst: vec![desc].into_boxed_slice(),
								len: size.into(),
								next_write: 0,
							};

							Ok(BufferToken {
								send_buff: Some(buffer),
								recv_buff: None,
								vq: self.clone(),
								ret_send: true,
								ret_recv: false,
								reusable: true,
							})
						}
						Err(vq_err) => Err(vq_err),
					},
					BuffSpec::Multiple(size_lst) => {
						let mut desc_lst: Vec<MemDescr> = Vec::with_capacity(size_lst.len());
						let mut len = 0usize;

						for size in size_lst {
							match self.mem_pool().pull(*size) {
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
							vq: self.clone(),
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
							desc_lst.push(self.mem_pool().pull_untracked(*size));
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
							vq: self.clone(),
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
					BuffSpec::Single(size) => match self.mem_pool().pull(size) {
						Ok(desc) => {
							let buffer = Buffer::Single {
								desc_lst: vec![desc].into_boxed_slice(),
								len: size.into(),
								next_write: 0,
							};

							Ok(BufferToken {
								send_buff: None,
								recv_buff: Some(buffer),
								vq: self.clone(),
								ret_send: false,
								ret_recv: true,
								reusable: true,
							})
						}
						Err(vq_err) => Err(vq_err),
					},
					BuffSpec::Multiple(size_lst) => {
						let mut desc_lst: Vec<MemDescr> = Vec::with_capacity(size_lst.len());
						let mut len = 0usize;

						for size in size_lst {
							match self.mem_pool().pull(*size) {
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
							vq: self.clone(),
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
							desc_lst.push(self.mem_pool().pull_untracked(*size));
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
							vq: self.clone(),
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
						let send_buff = match self.mem_pool().pull(send_size) {
							Ok(send_desc) => Some(Buffer::Single {
								desc_lst: vec![send_desc].into_boxed_slice(),
								len: send_size.into(),
								next_write: 0,
							}),
							Err(vq_err) => return Err(vq_err),
						};

						let recv_buff = match self.mem_pool().pull(recv_size) {
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
							vq: self.clone(),
							ret_send: true,
							ret_recv: true,
							reusable: true,
						})
					}
					(BuffSpec::Single(send_size), BuffSpec::Multiple(recv_size_lst)) => {
						let send_buff = match self.mem_pool().pull(send_size) {
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
							match self.mem_pool().pull(*size) {
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
							vq: self.clone(),
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
							match self.mem_pool().pull(*size) {
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
							match self.mem_pool().pull(*size) {
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
							vq: self.clone(),
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
							match self.mem_pool().pull(*size) {
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

						let recv_buff = match self.mem_pool().pull(recv_size) {
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
							vq: self.clone(),
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
							send_desc_lst.push(self.mem_pool().pull_untracked(*size));
							send_len += usize::from(*size);
						}

						let mut recv_desc_lst: Vec<MemDescr> =
							Vec::with_capacity(recv_size_lst.len());
						let mut recv_len = 0usize;

						for size in recv_size_lst {
							// As the indirect list does only consume one descriptor for the
							// control descriptor, the actual list is untracked
							recv_desc_lst.push(self.mem_pool().pull_untracked(*size));
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
							vq: self.clone(),
							ret_send: true,
							ret_recv: true,
							reusable: true,
						})
					}
					(BuffSpec::Indirect(_), BuffSpec::Single(_))
					| (BuffSpec::Indirect(_), BuffSpec::Multiple(_)) => Err(VirtqError::BufferInWithDirect),
					(BuffSpec::Single(_), BuffSpec::Indirect(_))
					| (BuffSpec::Multiple(_), BuffSpec::Indirect(_)) => Err(VirtqError::BufferInWithDirect),
				}
			}
		}
	}
}

/// These methods are an implementation detail and are meant only for consumption by the default method
/// implementations in [Virtq].
trait VirtqPrivate {
	fn create_indirect_ctrl(
		&self,
		send: Option<&Vec<MemDescr>>,
		recv: Option<&Vec<MemDescr>>,
	) -> Result<MemDescr, VirtqError>;

	fn mem_pool(&self) -> Rc<MemPool>;
}

/// Allows to check, if a given structure crosses a physical page boundary.
/// Returns true, if the structure does NOT cross a boundary or crosses only
/// contiguous physical page boundaries.
///
/// Structures provided to the Queue must pass this test, otherwise the queue
/// currently panics.
pub fn check_bounds<T: AsSliceU8>(data: &T) -> bool {
	let slice = data.as_slice_u8();

	let start_virt = ptr::from_ref(slice.first().unwrap()).addr();
	let end_virt = ptr::from_ref(slice.last().unwrap()).addr();
	let end_phy_calc = paging::virt_to_phys(VirtAddr::from(start_virt)) + (slice.len() - 1);
	let end_phy = paging::virt_to_phys(VirtAddr::from(end_virt));

	end_phy == end_phy_calc
}

/// Allows to check, if a given slice crosses a physical page boundary.
/// Returns true, if the slice does NOT cross a boundary or crosses only
/// contiguous physical page boundaries.
/// Slice MUST come from a boxed value. Otherwise the slice might be moved and
/// the test of this function is not longer valid.
///
/// This check is especially useful if one wants to check if slices
/// into which the queue will destructure a structure are valid for the queue.
///
/// Slices provided to the Queue must pass this test, otherwise the queue
/// currently panics.
pub fn check_bounds_slice(slice: &[u8]) -> bool {
	let start_virt = ptr::from_ref(slice.first().unwrap()).addr();
	let end_virt = ptr::from_ref(slice.last().unwrap()).addr();
	let end_phy_calc = paging::virt_to_phys(VirtAddr::from(start_virt)) + (slice.len() - 1);
	let end_phy = paging::virt_to_phys(VirtAddr::from(end_virt));

	end_phy == end_phy_calc
}

/// Frees memory regions gained access to via `Transfer.ret_raw()`.
pub fn free_raw(ptr: *mut u8, len: usize) {
	crate::mm::deallocate(VirtAddr::from(ptr as usize), len);
}

/// Dispatches a batch of TransferTokens. The actual behaviour depends on the respective
/// virtqueue implementation. Please see the respective docs for details.
///
/// **INFO:**
/// Due to the missing HashMap implementation in the kernel, this function currently uses a nested
/// for-loop. The first iteration is over the number if dispatched tokens. Inside this loop, the
/// function iterates over a list of all already "used" virtqueues. If the given token belongs to an
/// existing queue it is inserted into the corresponding list of tokens, if it belongs to no queue,
/// a new entry in the "used" virtqueues list is made.
/// This procedure can possibly be very slow.
///
/// The `notif` parameter indicates if the driver wants to have a notification for this specific
/// transfer. This is only for performance optimization. As it is NOT ensured, that the device sees the
/// updated notification flags before finishing transfers!
pub fn dispatch_batch(tkns: Vec<TransferToken>, notif: bool) {
	let mut used_vqs: Vec<(Rc<dyn Virtq>, Vec<TransferToken>)> = Vec::new();

	// Sort the TransferTokens depending in the queue their coming from.
	// then call dispatch_batch of that queue
	for tkn in tkns {
		let index = tkn.get_vq().index();
		let mut used = false;
		let mut index_used = 0usize;

		for (pos, (vq, _)) in used_vqs.iter_mut().enumerate() {
			if index == vq.index() {
				index_used = pos;
				used = true;
				break;
			}
		}

		if used {
			let (_, tkn_lst) = &mut used_vqs[index_used];
			tkn_lst.push(tkn);
		} else {
			let mut new_tkn_lst = Vec::new();
			let vq = tkn.get_vq();
			new_tkn_lst.push(tkn);

			used_vqs.push((vq, new_tkn_lst))
		}
	}

	for (vq_ref, tkn_lst) in used_vqs {
		vq_ref.dispatch_batch(tkn_lst, notif);
	}
}

/// Dispatches a batch of TransferTokens. The Transfers will be placed in to the `await_queue`
/// upon finish.
///
/// **INFO:**
/// Due to the missing HashMap implementation in the kernel, this function currently uses a nested
/// for-loop. The first iteration is over the number if dispatched tokens. Inside this loop, the
/// function iterates over a list of all already "used" virtqueues. If the given token belongs to an
/// existing queue it is inserted into the corresponding list of tokens, if it belongs to no queue,
/// a new entry in the "used" virtqueues list is made.
/// This procedure can possibly be very slow.
///
/// The `notif` parameter indicates if the driver wants to have a notification for this specific
/// transfer. This is only for performance optimization. As it is NOT ensured, that the device sees the
/// updated notification flags before finishing transfers!
pub fn dispatch_batch_await(tkns: Vec<TransferToken>, await_queue: BufferTokenSender, notif: bool) {
	let mut used_vqs: Vec<(Rc<dyn Virtq>, Vec<TransferToken>)> = Vec::new();

	// Sort the TransferTokens depending in the queue their coming from.
	// then call dispatch_batch of that queue
	for tkn in tkns {
		let index = tkn.get_vq().index();
		let mut used = false;
		let mut index_used = 0usize;

		for (pos, (vq, _)) in used_vqs.iter_mut().enumerate() {
			if index == vq.index() {
				index_used = pos;
				used = true;
				break;
			}
		}

		if used {
			let (_, tkn_lst) = &mut used_vqs[index_used];
			tkn_lst.push(tkn);
		} else {
			let mut new_tkn_lst = Vec::new();
			let vq = tkn.get_vq();
			new_tkn_lst.push(tkn);

			used_vqs.push((vq, new_tkn_lst))
		}
	}

	for (vq, tkn_lst) in used_vqs {
		vq.dispatch_batch_await(tkn_lst, await_queue.clone(), notif);
	}
}

/// The trait needs to be implemented for
/// structures which are to be used to write data into buffers of a [BufferToken] via [BufferToken::write] or
/// `BufferToken.write_seq()`.
///
/// **INFO:*
/// The trait provides a decent default implementation. Please look at the code for details.
/// The provided default implementation computes the size of the given structure via `core::mem::size_of_val(&self)`
/// and then casts the given `*const Self` pointer of the structure into an `*const u8`.
///
/// Users must be really careful, and check, whether the memory representation of the given structure equals
/// the representation the device expects. It is advised to only use `#[repr(C)]` and to check the output
/// of `as_slice_u8` and `as_slice_u8_mut`.
pub trait AsSliceU8 {
	/// Retruns the size of the structure
	///
	/// In case of an unsized structure, the function should returns
	/// the exact value of the structure.
	fn len(&self) -> usize {
		core::mem::size_of_val(self)
	}

	/// Returns a slice of the given structure.
	///
	/// ** WARN:**
	/// * The slice must be little endian coded in order to be understood by the device
	/// * The slice must serialize the actual structure the device expects, as the queue will use
	///   the addresses of the slice in order to refer to the structure.
	fn as_slice_u8(&self) -> &[u8] {
		unsafe { core::slice::from_raw_parts(ptr::from_ref(self) as *const u8, self.len()) }
	}

	/// Returns a mutable slice of the given structure.
	///
	/// ** WARN:**
	/// * The slice must be little endian coded in order to be understood by the device
	/// * The slice must serialize the actual structure the device expects, as the queue will use
	///   the addresses of the slice in order to refer to the structure.
	fn as_slice_u8_mut(&mut self) -> &mut [u8] {
		unsafe { core::slice::from_raw_parts_mut(ptr::from_mut(self) as *mut u8, self.len()) }
	}
}

/// The struct represents buffers which are ready to be send via the
/// virtqueue. Buffers can no longer be written or retrieved.
pub struct TransferToken {
	/// Must be some in order to prevent drop
	/// upon reuse.
	buff_tkn: Option<BufferToken>,
	/// Structure which allows to await Transfers
	/// If Some, finished TransferTokens will be placed here
	/// as finished `Transfers`. If None, only the state
	/// of the Token will be changed.
	await_queue: Option<BufferTokenSender>,
}

/// Public Interface for TransferToken
impl TransferToken {
	/// Returns a reference to the holding virtqueue
	pub fn get_vq(&self) -> Rc<dyn Virtq> {
		// Unwrapping is okay here, as TransferToken must hold a BufferToken
		Rc::clone(&self.buff_tkn.as_ref().unwrap().vq)
	}

	/// Dispatches a TransferToken and awaits it at the specified queue.
	///
	/// The `notif` parameter indicates if the driver wants to have a notification for this specific
	/// transfer. This is only for performance optimization. As it is NOT ensured, that the device sees the
	/// updated notification flags before finishing transfers!
	pub fn dispatch_await(mut self, await_queue: BufferTokenSender, notif: bool) {
		self.await_queue = Some(await_queue.clone());

		self.get_vq().dispatch(self, notif);
	}

	/// Dispatches the provided TransferToken to the respective queue.
	///
	/// The `notif` parameter indicates if the driver wants to have a notification for this specific
	/// transfer. This is only for performance optimization. As it is NOT ensured, that the device sees the
	/// updated notification flags before finishing transfers!
	pub fn dispatch(self, notif: bool) {
		self.get_vq().dispatch(self, notif);
	}

	/// Dispatches the provided TransferToken to the respectuve queue and does
	/// return when, the queue finished the transfer.
	///
	/// The returned [BufferToken] can be reused, copied from
	/// or return the underlying buffers.
	///
	/// **INFO:**
	/// Currently this function is constantly polling the queue while keeping the notifications disabled.
	/// Upon finish notifications are enabled again.
	pub fn dispatch_blocking(self) -> Result<Box<BufferToken>, VirtqError> {
		let vq = self.get_vq();
		let (sender, receiver) = async_channel::bounded(1);
		self.dispatch_await(sender, false);

		vq.disable_notifs();

		let result: Box<BufferToken>;
		// Keep Spinning until the receive queue is filled
		loop {
			match receiver.try_recv() {
				Ok(buffer_tkn) => {
					result = buffer_tkn;
					break;
				}
				Err(TryRecvError::Closed) => return Err(VirtqError::General),
				Err(TryRecvError::Empty) => vq.poll(),
			}
		}

		vq.enable_notifs();

		Ok(result)
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
	send_buff: Option<Buffer>,
	//send_desc_lst: Option<Vec<usize>>,
	recv_buff: Option<Buffer>,
	//recv_desc_lst: Option<Vec<usize>>,
	vq: Rc<dyn Virtq>,
	/// Indicates whether the buff is returnable
	ret_send: bool,
	ret_recv: bool,
	/// Indicates if the token is allowed
	/// to be reused.
	reusable: bool,
}

// Private interface of BufferToken
impl BufferToken {
	/// Returns the overall number of descriptors.
	fn num_descr(&self) -> u16 {
		let mut len = 0;

		if let Some(buffer) = &self.recv_buff {
			len += buffer.num_descr();
		}

		if let Some(buffer) = &self.send_buff {
			len += buffer.num_descr();
		}
		len
	}

	/// Returns the number of descritprors that will be placed in the queue.
	/// This number can differ from the `BufferToken.num_descr()` function value
	/// as indirect buffers only consume one descriptor in the queue, but can have
	/// more descriptors that are accessible via the descriptor in the queue.
	fn num_consuming_descr(&self) -> u16 {
		let mut len = 0;

		if let Some(buffer) = &self.send_buff {
			match buffer.get_ctrl_desc() {
				Some(_) => len += 1,
				None => len += buffer.num_descr(),
			}
		}

		if let Some(buffer) = &self.recv_buff {
			match buffer.get_ctrl_desc() {
				Some(_) => len += 1,
				None => len += buffer.num_descr(),
			}
		}
		len
	}

	/// Resets all properties from the previous transfer.
	///
	/// Includes:
	/// * Resetting the write status inside the MemDescr. -> Allowing to rewrite the buffers
	/// * Resetting the MemDescr length at initialization. This length might be reduced upon writes
	///   of the driver or the device.
	/// * Erazing all memory areas with zeros
	fn reset_purge(mut self) -> Self {
		let mut ctrl_desc_cnt = 0usize;

		if let Some(buff) = self.send_buff.as_mut() {
			buff.reset_write();
			let mut init_buff_len = 0usize;

			match buff.get_ctrl_desc_mut() {
				Some(ctrl_desc) => {
					let ind_desc_lst = unsafe {
						let size = core::mem::size_of::<Descriptor>();
						core::slice::from_raw_parts_mut(
							ctrl_desc.ptr as *mut Descriptor,
							ctrl_desc.len / size,
						)
					};

					for desc in buff.as_mut_slice() {
						desc.len = desc._init_len;
						// This is fine as the length of the descriptors is restricted
						// by u32::MAX (see also Bytes::new())
						ind_desc_lst[ctrl_desc_cnt].len = desc._init_len as u32;
						ctrl_desc_cnt += 1;
						init_buff_len += desc._init_len;

						// Resetting written memory
						for byte in desc.deref_mut() {
							*byte = 0;
						}
					}
				}
				None => {
					for desc in buff.as_mut_slice() {
						desc.len = desc._init_len;
						init_buff_len += desc._init_len;

						// Resetting written memory
						for byte in desc.deref_mut() {
							*byte = 0;
						}
					}
				}
			}

			buff.reset_len(init_buff_len);
		}

		if let Some(buff) = self.recv_buff.as_mut() {
			buff.reset_write();
			let mut init_buff_len = 0usize;

			match buff.get_ctrl_desc_mut() {
				Some(ctrl_desc) => {
					let ind_desc_lst = unsafe {
						let size = core::mem::size_of::<Descriptor>();
						core::slice::from_raw_parts_mut(
							ctrl_desc.ptr as *mut Descriptor,
							ctrl_desc.len / size,
						)
					};

					for desc in buff.as_mut_slice() {
						desc.len = desc._init_len;
						// This is fine as the length of the descriptors is restricted
						// by u32::MAX (see also Bytes::new())
						ind_desc_lst[ctrl_desc_cnt].len = desc._init_len as u32;
						ctrl_desc_cnt += 1;
						init_buff_len += desc._init_len;

						// Resetting written memory
						for byte in desc.deref_mut() {
							*byte = 0;
						}
					}
				}
				None => {
					for desc in buff.as_mut_slice() {
						desc.len = desc._init_len;
						init_buff_len += desc._init_len;

						// Resetting written memory
						for byte in desc.deref_mut() {
							*byte = 0;
						}
					}
				}
			}

			buff.reset_len(init_buff_len);
		}
		self
	}

	/// Resets all properties from the previous transfer.
	///
	/// Includes:
	/// * Resetting the write status inside the MemDescr. -> Allowing to rewrite the buffers
	/// * Resetting the MemDescr length at initialization. This length might be reduced upon writes
	///   of the driver or the device.
	pub fn reset(mut self) -> Self {
		let mut ctrl_desc_cnt = 0usize;

		if let Some(buff) = self.send_buff.as_mut() {
			buff.reset_write();
			let mut init_buff_len = 0usize;

			match buff.get_ctrl_desc_mut() {
				Some(ctrl_desc) => {
					let ind_desc_lst = unsafe {
						let size = core::mem::size_of::<Descriptor>();
						core::slice::from_raw_parts_mut(
							ctrl_desc.ptr as *mut Descriptor,
							ctrl_desc.len / size,
						)
					};

					for desc in buff.as_mut_slice() {
						desc.len = desc._init_len;
						// This is fine as the length of the descriptors is restricted
						// by u32::MAX (see also Bytes::new())
						ind_desc_lst[ctrl_desc_cnt].len = desc._init_len as u32;
						ctrl_desc_cnt += 1;
						init_buff_len += desc._init_len;
					}
				}
				None => {
					for desc in buff.as_mut_slice() {
						desc.len = desc._init_len;
						init_buff_len += desc._init_len;
					}
				}
			}

			buff.reset_len(init_buff_len);
		}

		if let Some(buff) = self.recv_buff.as_mut() {
			buff.reset_write();
			let mut init_buff_len = 0usize;

			match buff.get_ctrl_desc_mut() {
				Some(ctrl_desc) => {
					let ind_desc_lst = unsafe {
						let size = core::mem::size_of::<Descriptor>();
						core::slice::from_raw_parts_mut(
							ctrl_desc.ptr as *mut Descriptor,
							ctrl_desc.len / size,
						)
					};

					for desc in buff.as_mut_slice() {
						desc.len = desc._init_len;
						// This is fine as the length of the descriptors is restricted
						// by u32::MAX (see also Bytes::new())
						ind_desc_lst[ctrl_desc_cnt].len = desc._init_len as u32;
						ctrl_desc_cnt += 1;
						init_buff_len += desc._init_len;
					}
				}
				None => {
					for desc in buff.as_mut_slice() {
						desc.len = desc._init_len;
						init_buff_len += desc._init_len;
					}
				}
			}

			buff.reset_len(init_buff_len);
		}
		self
	}
}

// Public interface of BufferToken
impl BufferToken {
	/// Restricts the size of a given BufferToken. One must specify either a `new_send_len` or/and `new_recv_len`. If possible
	/// the function will restrict the respective buffers size to this value. This is especially useful if one has to provide the
	/// user-space or the device with a buffer and has already a free buffer at hand, which is to large. With this method the user
	/// of the buffer will only see the given sizes. Although the buffer is NOT reallocated.
	///
	/// **INFO:**
	/// * Upon Transfer.reuse() call the Buffers will restore their original size, which was provided at creation time!
	/// * Fails if buffer to be restricted is non existing -> VirtqError::NoBufferAvail
	/// * Fails if buffer to be restricted is to small (i.e. `buff.len < new_len`) -> VirtqError::General
	pub fn restr_size(
		&mut self,
		new_send_len: Option<usize>,
		new_recv_len: Option<usize>,
	) -> Result<(usize, usize), VirtqError> {
		let send_len = match new_send_len {
			Some(new_len) => {
				match self.send_buff.as_mut() {
					Some(send_buff) => {
						let mut ctrl_desc_cnt = 0usize;

						match send_buff.get_ctrl_desc() {
							None => {
								if send_buff.len() < new_len {
									return Err(VirtqError::General);
								} else {
									let mut len_now = 0usize;
									let mut rest_zero = false;
									for desc in send_buff.as_mut_slice() {
										len_now += desc.len;

										if len_now >= new_len && !rest_zero {
											desc.len -= len_now - new_len;
											rest_zero = true;
										} else if rest_zero {
											desc.len = 0;
										}
									}

									send_buff.restr_len(new_len);
									new_len
								}
							}
							Some(ctrl_desc) => {
								if send_buff.len() < new_len {
									return Err(VirtqError::General);
								} else {
									let ind_desc_lst = unsafe {
										let size = core::mem::size_of::<Descriptor>();
										core::slice::from_raw_parts_mut(
											ctrl_desc.ptr as *mut Descriptor,
											ctrl_desc.len / size,
										)
									};

									let mut len_now = 0usize;
									let mut rest_zero = false;

									for desc in send_buff.as_mut_slice() {
										len_now += desc.len;

										if len_now >= new_len && !rest_zero {
											desc.len -= len_now - new_len;
											// As u32 is save here as all buffers length is restricted by u32::MAX
											ind_desc_lst[ctrl_desc_cnt].len -=
												(len_now - new_len) as u32;

											rest_zero = true;
										} else if rest_zero {
											desc.len = 0;
											ind_desc_lst[ctrl_desc_cnt].len = 0;
										}
										ctrl_desc_cnt += 1;
									}

									send_buff.restr_len(new_len);
									new_len
								}
							}
						}
					}
					None => return Err(VirtqError::NoBufferAvail),
				}
			}
			None => match self.send_buff.as_mut() {
				Some(send_buff) => send_buff.len(),
				None => 0,
			},
		};

		let recv_len = match new_recv_len {
			Some(new_len) => {
				match self.recv_buff.as_mut() {
					Some(recv_buff) => {
						let mut ctrl_desc_cnt = 0usize;

						match recv_buff.get_ctrl_desc() {
							None => {
								if recv_buff.len() < new_len {
									return Err(VirtqError::General);
								} else {
									let mut len_now = 0usize;
									let mut rest_zero = false;
									for desc in recv_buff.as_mut_slice() {
										len_now += desc.len;

										if len_now >= new_len && !rest_zero {
											desc.len -= len_now - new_len;
											rest_zero = true;
										} else if rest_zero {
											desc.len = 0;
										}
									}

									recv_buff.restr_len(new_len);
									new_len
								}
							}
							Some(ctrl_desc) => {
								if recv_buff.len() < new_len {
									return Err(VirtqError::General);
								} else {
									let ind_desc_lst = unsafe {
										let size = core::mem::size_of::<Descriptor>();
										core::slice::from_raw_parts_mut(
											ctrl_desc.ptr as *mut Descriptor,
											ctrl_desc.len / size,
										)
									};

									let mut len_now = 0usize;
									let mut rest_zero = false;

									for desc in recv_buff.as_mut_slice() {
										len_now += desc.len;

										if len_now >= new_len && !rest_zero {
											desc.len -= len_now - new_len;
											// As u32 is save here as all buffers length is restricted by u32::MAX
											ind_desc_lst[ctrl_desc_cnt].len -=
												(len_now - new_len) as u32;

											rest_zero = true;
										} else if rest_zero {
											desc.len = 0;
											ind_desc_lst[ctrl_desc_cnt].len = 0;
										}
										ctrl_desc_cnt += 1;
									}

									recv_buff.restr_len(new_len);
									new_len
								}
							}
						}
					}
					None => return Err(VirtqError::NoBufferAvail),
				}
			}
			None => match self.recv_buff.as_mut() {
				Some(recv_buff) => recv_buff.len(),
				None => 0,
			},
		};

		Ok((send_len, recv_len))
	}

	/// Returns the overall number of bytes in the send and receive memory area
	/// respectively for this BufferToken
	pub fn len(&self) -> (usize, usize) {
		match (self.send_buff.as_ref(), self.recv_buff.as_ref()) {
			(Some(send_buff), Some(recv_buff)) => (send_buff.len(), recv_buff.len()),
			(Some(send_buff), None) => (send_buff.len(), 0),
			(None, Some(recv_buff)) => (0, recv_buff.len()),
			(None, None) => unreachable!("Empty BufferToken not allowed!"),
		}
	}

	/// Returns the underlying raw pointers to the user accessible memory hold by the Buffertoken. This is mostly
	/// useful in order to provide the user space with pointers to write to. Return tuple has the form
	/// (`pointer_to_mem_area`, `length_of_accessible_mem_area`).
	///
	/// **INFO:**
	///
	/// The length of the given memory area MUST NOT express the actual allocated memory area. This is due to the behaviour
	/// of the allocation function. Although it is ensured that the allocated memory area length is always larger or equal
	/// to the "accessible memory area". Hence one MUST NOT use this information in order to deallocate the underlying memory.
	/// If this is wanted the savest way is to simpyl drop the BufferToken.
	///
	///
	/// **WARN:** The Buffertoken is controlling the memory and must not be dropped as long as
	/// userspace has access to it!
	pub fn raw_ptrs(
		&mut self,
	) -> (
		Option<Box<[(*mut u8, usize)]>>,
		Option<Box<[(*mut u8, usize)]>>,
	) {
		let mut send_ptrs = Vec::new();
		let mut recv_ptrs = Vec::new();

		if let Some(buff) = self.send_buff.as_mut() {
			for desc in buff.as_slice() {
				send_ptrs.push((desc.ptr, desc.len()));
			}
		}

		if let Some(buff) = self.recv_buff.as_ref() {
			for desc in buff.as_slice() {
				recv_ptrs.push((desc.ptr, desc.len()));
			}
		}

		match (send_ptrs.is_empty(), recv_ptrs.is_empty()) {
			(true, true) => unreachable!("Empty transfer, Not allowed"),
			(false, true) => (Some(send_ptrs.into_boxed_slice()), None),
			(true, false) => (None, Some(recv_ptrs.into_boxed_slice())),
			(false, false) => (
				Some(send_ptrs.into_boxed_slice()),
				Some(recv_ptrs.into_boxed_slice()),
			),
		}
	}

	/// Returns a vector of immutable slices to the underlying memory areas.
	///
	/// The vectors contain the slices in creation order.
	/// E.g.:
	/// * Driver creates buffer as
	///   * send buffer: 50 bytes, 60 bytes
	///   * receive buffer: 10 bytes
	/// * The return tuple will be:
	///  * `(Some(vec[50, 60]), Some(vec[10]))`
	///  * Where 50 refers to a slice of u8 of length 50.
	///    The other numbers follow the same principle.
	pub fn as_slices(&self) -> Result<(Option<Vec<&[u8]>>, Option<Vec<&[u8]>>), VirtqError> {
		// Unwrapping is okay here, as TransferToken must hold a BufferToken
		let send_data = match &self.send_buff {
			Some(buff) => {
				let mut arr = Vec::with_capacity(buff.as_slice().len());

				for desc in buff.as_slice() {
					arr.push(desc.deref())
				}

				Some(arr)
			}
			None => None,
		};

		let recv_data = match &self.recv_buff {
			Some(buff) => {
				let mut arr = Vec::with_capacity(buff.as_slice().len());

				for desc in buff.as_slice() {
					arr.push(desc.deref())
				}

				Some(arr)
			}
			None => None,
		};

		Ok((send_data, recv_data))
	}

	/// Returns a vector of mutable slices to the underlying memory areas.
	///
	/// The vectors contain the slices in creation order.
	/// E.g.:
	/// * Driver creates buffer as
	///   * send buffer: 50 bytes, 60 bytes
	///   * receive buffer: 10 bytes
	/// * The return tuple will be:
	///  * `(Some(vec[50, 60]), Some(vec[10]))`
	///  * Where 50 refers to a slice of u8 of length 50.
	///    The other numbers follow the same principle.
	pub fn as_slices_mut(
		&mut self,
	) -> Result<(Option<Vec<&mut [u8]>>, Option<Vec<&mut [u8]>>), VirtqError> {
		let (send_buff, recv_buff) = {
			let BufferToken {
				send_buff,
				recv_buff,
				..
			} = self;

			(send_buff.as_mut(), recv_buff.as_mut())
		};

		// Unwrapping is okay here, as TransferToken must hold a BufferToken
		let send_data = match send_buff {
			Some(buff) => {
				let mut arr = Vec::with_capacity(buff.as_slice().len());

				for desc in buff.as_mut_slice() {
					arr.push(desc.deref_mut())
				}

				Some(arr)
			}
			None => None,
		};

		let recv_data = match recv_buff {
			Some(buff) => {
				let mut arr = Vec::with_capacity(buff.as_slice().len());

				for desc in buff.as_mut_slice() {
					arr.push(desc.deref_mut())
				}

				Some(arr)
			}
			None => None,
		};

		Ok((send_data, recv_data))
	}

	/// Writes the provided datastructures into the respective buffers. `K` into `self.send_buff` and `H` into
	/// `self.recv_buff`.
	/// If the provided datastructures do not "fit" into the respective buffers, the function will return an error. Even
	/// if only one of the two structures is to large.
	/// The same error will be triggered in case the respective buffer wasn't even existing, as not all transfers consist
	/// of send and recv buffers.
	///
	/// This write DOES NOT reduce the overall size of the buffer to length_of(`K` or `H`). The device will observe the length of
	/// the buffer as given by `BufferToken.len()`.
	/// Use `BufferToken.restr_size()` in order to change this property.
	///
	///
	/// # Detailed Description
	/// The respective send and recv buffers (see [BufferToken] docs for details on buffers) consist of multiple
	/// descriptors.
	/// The `write()` function does NOT take into account the distinct descriptors of a buffer but treats the buffer as a single continuous
	/// memory element and as a result writes `T` or `H` as a slice of bytes into this memory.
	pub fn write<K: AsSliceU8 + ?Sized, H: AsSliceU8 + ?Sized>(
		mut self,
		send: Option<&K>,
		recv: Option<&H>,
	) -> Result<TransferToken, VirtqError> {
		if let Some(data) = send {
			match self.send_buff.as_mut() {
				Some(buff) => {
					if buff.len() < data.as_slice_u8().len() {
						return Err(VirtqError::WriteTooLarge);
					} else {
						let data_slc = data.as_slice_u8();
						let mut from = 0usize;

						for i in 0..usize::from(buff.num_descr()) {
							// Must check array boundaries, as allocated buffer might be larger
							// than actual data to be written.
							let to = if (buff.as_slice()[i].len() + from) > data_slc.len() {
								data_slc.len()
							} else {
								from + buff.as_slice()[i].len()
							};

							// Unwrapping is okay here as sizes are checked above
							from += buff.next_write(&data_slc[from..to]).unwrap();
						}
					}
				}
				None => return Err(VirtqError::NoBufferAvail),
			}
		}

		if let Some(data) = recv {
			match self.recv_buff.as_mut() {
				Some(buff) => {
					let data_slc = data.as_slice_u8();

					if buff.len() < data_slc.len() {
						return Err(VirtqError::WriteTooLarge);
					} else {
						let mut from = 0usize;

						for i in 0..usize::from(buff.num_descr()) {
							// Must check array boundaries, as allocated buffer might be larger
							// than actual data to be written.
							let to = if (buff.as_slice()[i].len() + from) > data_slc.len() {
								data_slc.len()
							} else {
								from + buff.as_slice()[i].len()
							};

							// Unwrapping is okay here as sizes are checked above
							from += buff.next_write(&data_slc[from..to]).unwrap();
						}
					}
				}
				None => return Err(VirtqError::NoBufferAvail),
			}
		}

		Ok(TransferToken {
			buff_tkn: Some(self),
			await_queue: None,
		})
	}

	/// Writes `K` or `H` respectively into the next buffer descriptor.
	/// Will return an VirtqError, if the `mem_size_of_val(K or H)` is larger than the respective buffer descriptor.
	///
	/// # Detailed Description
	/// A write procedure to the buffers of the BufferToken could look like the following:
	///
	/// * First Write: `write_seq(Some(8 bytes), Some(3 bytes))`:
	///   * Will result in 8 bytes written to the first buffer descriptor of the send buffer and 3 bytes written to the first buffer descriptor of the recv buffer.
	/// * Second Write: `write_seq(None, Some(4 bytes))`:
	///   * Will result in 4 bytes written to the second buffer descriptor of the recv buffer. Nothing is written into the second buffer descriptor.
	/// * Third Write: `write_seq(Some(10 bytes, Some(4 bytes))`:
	///   * Will result in 10 bytes written to the second buffer descriptor of the send buffer and 4 bytes written to the third buffer descriptor of the recv buffer.
	pub fn write_seq<K: AsBytes, H: AsBytes + ?Sized>(
		mut self,
		send_seq: Option<&K>,
		recv_seq: Option<&H>,
	) -> Result<Self, VirtqError> {
		if let Some(data) = send_seq {
			match self.send_buff.as_mut() {
				Some(buff) => {
					match buff.next_write(data.as_bytes()) {
						Ok(_) => (), // Do nothing, write fitted inside descriptor and not to many writes to buffer happened
						Err(_) => {
							// Need no match here, as result is the same, but for the future one could
							// pass on the actual BufferError wrapped inside a VirtqError, for better recovery
							return Err(VirtqError::WriteTooLarge);
						}
					}
				}
				None => return Err(VirtqError::NoBufferAvail),
			}
		}

		if let Some(data) = recv_seq {
			match self.recv_buff.as_mut() {
				Some(buff) => {
					match buff.next_write(data.as_bytes()) {
						Ok(_) => (), // Do nothing, write fitted inside descriptor and not to many writes to buffer happened
						Err(_) => {
							// Need no match here, as result is the same, but for the future one could
							// pass on the actual BufferError wrapped inside a VirtqError, for better recovery
							return Err(VirtqError::WriteTooLarge);
						}
					}
				}
				None => return Err(VirtqError::NoBufferAvail),
			}
		}

		Ok(self)
	}

	/// Consumes the [BufferToken] and returns a [TransferToken], that can be used to actually start the transfer.
	///
	/// After this call, the buffers are no longer writable.
	pub fn provide(self) -> TransferToken {
		TransferToken {
			buff_tkn: Some(self),
			await_queue: None,
		}
	}
}

pub enum BufferType {
	Direct,
	Indirect,
}

/// Describes the type of a buffer and unifies them.
enum Buffer {
	/// A buffer consisting of a single [Memory Descriptor](MemDescr).
	Single {
		desc_lst: Box<[MemDescr]>,
		len: usize,
		next_write: usize,
	},
	/// A buffer consisting of a chain of [Memory Descriptors](MemDescr).
	/// Especially useful if one wants to send multiple structures to a device,
	/// as he can sequentially write (see [BufferToken] `write_seq()`)
	/// those structs into the descriptors.
	Multiple {
		desc_lst: Box<[MemDescr]>,
		len: usize,
		next_write: usize,
	},
	/// A buffer consisting of a single descriptor in the actual virtqueue,
	/// referencing a list of descriptors somewhere in memory.
	/// Especially useful of one wants to extend the capacity of the virtqueue.
	/// Also has the same advantages as a `Buffer::Multiple`.
	Indirect {
		desc_lst: Box<[MemDescr]>,
		ctrl_desc: MemDescr,
		len: usize,
		next_write: usize,
	},
}

// Private Interface of Buffer
impl Buffer {
	/// Resets the Buffers length to the given len. This MUST be the length at initialization.
	fn reset_len(&mut self, init_len: usize) {
		match self {
			Buffer::Single { len, .. }
			| Buffer::Multiple { len, .. }
			| Buffer::Indirect { len, .. } => *len = init_len,
		}
	}

	/// Restricts the Buffers length to the given len. This length MUST NOT be larger than the
	/// length at initialization or smaller-equal 0.
	fn restr_len(&mut self, new_len: usize) {
		match self {
			Buffer::Single { len, .. }
			| Buffer::Multiple { len, .. }
			| Buffer::Indirect { len, .. } => *len = new_len,
		}
	}

	/// Writes a given slice into a Descriptor element of a Buffer. Hereby the function ensures, that the
	/// slice fits into the memory area and that not to many writes already have happened.
	fn next_write(&mut self, slice: &[u8]) -> Result<usize, BufferError> {
		match self {
			Buffer::Single {
				desc_lst,
				next_write,
				..
			}
			| Buffer::Multiple {
				desc_lst,
				next_write,
				..
			}
			| Buffer::Indirect {
				desc_lst,
				next_write,
				..
			} => {
				if (desc_lst.len() - 1) < *next_write {
					Err(BufferError::ToManyWrites)
				} else if desc_lst.get(*next_write).unwrap().len() < slice.len() {
					Err(BufferError::WriteToLarge)
				} else {
					desc_lst[*next_write].deref_mut()[0..slice.len()].copy_from_slice(slice);
					*next_write += 1;

					Ok(slice.len())
				}
			}
		}
	}

	/// Resets the write status of a Buffertoken in order to be able to reuse a Buffertoken.
	fn reset_write(&mut self) {
		match self {
			Buffer::Single { next_write, .. }
			| Buffer::Multiple { next_write, .. }
			| Buffer::Indirect { next_write, .. } => *next_write = 0,
		}
	}

	/// This consumes the the given buffer and returns the raw information (i.e. a `*mut u8` and a `usize` inidacting the start and
	/// length of the buffers memory).
	///
	/// After this call the users is responsible for deallocating the given memory via the kernel `mem::dealloc` function.
	fn into_raw(self) -> Vec<(*mut u8, usize)> {
		match self {
			Buffer::Single { mut desc_lst, .. }
			| Buffer::Multiple { mut desc_lst, .. }
			| Buffer::Indirect { mut desc_lst, .. } => {
				let mut arr = Vec::with_capacity(desc_lst.len());

				for desc in desc_lst.iter_mut() {
					// Need to be a little careful here.
					desc.dealloc = Dealloc::Not;
					arr.push((desc.ptr, desc._mem_len));
				}
				arr
			}
		}
	}

	/// Returns a copy of the buffer.
	fn cpy(&self) -> Box<[u8]> {
		match &self {
			Buffer::Single { desc_lst, len, .. }
			| Buffer::Multiple { desc_lst, len, .. }
			| Buffer::Indirect { desc_lst, len, .. } => {
				let mut arr = Vec::with_capacity(*len);

				for desc in desc_lst.iter() {
					arr.append(&mut desc.cpy_into_vec());
				}
				arr.into_boxed_slice()
			}
		}
	}

	/// Returns a scattered copy of the buffer, which preserves the structure of the
	/// buffer being possibly split up between different descriptors.
	fn scat_cpy(&self) -> Vec<Box<[u8]>> {
		match &self {
			Buffer::Single { desc_lst, .. }
			| Buffer::Multiple { desc_lst, .. }
			| Buffer::Indirect { desc_lst, .. } => {
				let mut arr = Vec::with_capacity(desc_lst.len());

				for desc in desc_lst.iter() {
					arr.push(desc.cpy_into_box());
				}
				arr
			}
		}
	}

	/// Returns the number of usable descriptors inside a buffer.
	/// In case of Indirect Buffers this will return the number of
	/// descriptors inside the indirect descriptor table. As a result
	/// the return value most certainly IS NOT equall to the number of
	/// descriptors that will be placed inside the virtqueue.
	/// In order to retrieve this value, please use `BufferToken.num_consuming_desc()`.
	fn num_descr(&self) -> u16 {
		match &self {
			Buffer::Single { desc_lst, .. }
			| Buffer::Multiple { desc_lst, .. }
			| Buffer::Indirect { desc_lst, .. } => u16::try_from(desc_lst.len()).unwrap(),
		}
	}

	/// Returns the overall number of bytes in this Buffer.
	///
	/// In case of a Indirect descriptor, this describes the accumulated length of the memory area of the descriptors
	/// inside the indirect descriptor list. NOT the length of the memory area of the indirect descriptor placed in the actual
	/// descriptor area!
	fn len(&self) -> usize {
		match &self {
			Buffer::Single { len, .. }
			| Buffer::Multiple { len, .. }
			| Buffer::Indirect { len, .. } => *len,
		}
	}

	/// Returns the complete Buffer as a mutable slice of MemDescr, which themselves deref into a `&mut [u8]`.
	///
	/// As Buffers are able to consist of multiple descriptors
	/// this will return one element
	/// (`&mut [u8]`) for each descriptor.
	fn as_mut_slice(&mut self) -> &mut [MemDescr] {
		match self {
			Buffer::Single { desc_lst, .. }
			| Buffer::Multiple { desc_lst, .. }
			| Buffer::Indirect { desc_lst, .. } => desc_lst.as_mut(),
		}
	}

	/// Returns the complete Buffer as a slice of MemDescr, which themselves deref into a `&[u8]`.
	///
	/// As Buffers are able to consist of multiple descriptors
	/// this will return one element
	/// (`&[u8]`) for each descriptor.
	fn as_slice(&self) -> &[MemDescr] {
		match self {
			Buffer::Single { desc_lst, .. } => desc_lst.as_ref(),
			Buffer::Multiple { desc_lst, .. } => desc_lst.as_ref(),
			Buffer::Indirect { desc_lst, .. } => desc_lst.as_ref(),
		}
	}

	/// Returns a reference to the buffers ctrl descriptor if available.
	fn get_ctrl_desc(&self) -> Option<&MemDescr> {
		if let Buffer::Indirect { ctrl_desc, .. } = self {
			Some(ctrl_desc)
		} else {
			None
		}
	}

	/// Returns a mutable reference to the buffers ctrl descriptor if available.
	fn get_ctrl_desc_mut(&mut self) -> Option<&mut MemDescr> {
		if let Buffer::Indirect { ctrl_desc, .. } = self {
			Some(ctrl_desc)
		} else {
			None
		}
	}

	/// Returns true if the buffer is an indirect one
	fn is_indirect(&self) -> bool {
		matches!(self, Buffer::Indirect { .. })
	}
}

/// Describes a chunk of heap allocated memory. This memory chunk will
/// be valid until this descriptor is dropped.
///
/// **Detailed INFOS:**
/// * Sometimes it is necessary to refer to some memory areas which are not
///   controlled by the kernel space or rather by someone else. In these
///   cases the `MemDesc` field `dealloc: bool` allows to prevent the deallocation
///   during drop of the object.
struct MemDescr {
	/// Points to the controlled memory area
	ptr: *mut u8,
	/// Defines the len of the memory area that is accessible by users
	/// Can change after the device wrote to the memory area partially.
	/// Hence, this always defines the length of the memory area that has
	/// useful information or is accessible.
	len: usize,
	/// Defines the len of the memory area that is accessible by users
	/// This field is needed as the `MemDescr.len` field might change
	/// after writes of the device, but the Descriptors need to be reset
	/// in case they are reused. So the initial length must be preserved.
	_init_len: usize,
	/// Defines the length of the controlled memory area
	/// starting a `ptr: *mut u8`. Never Changes.
	_mem_len: usize,
	/// If `id == None` this is an untracked memory descriptor
	/// * Meaining: The descriptor does NOT count as a descriptor
	///   taken from the [MemPool].
	id: Option<MemDescrId>,
	/// Refers to the controlling [memory pool](MemPool)
	pool: Rc<MemPool>,
	/// Controls whether the memory area is deallocated
	/// upon drop.
	/// * Should NEVER be set to true, when false.
	///   * As false will be set after creation and indicates
	///     that someone else is "controlling" area and takes
	///     of deallocation.
	/// * Default is true.
	dealloc: Dealloc,
}

impl MemDescr {
	/// Provides a handle to the given memory area by
	/// giving a Box ownership to it.
	fn into_vec(mut self) -> Vec<u8> {
		// Prevent double frees, as ownership will be tracked by
		// Box from now on.
		self.dealloc = Dealloc::Not;

		unsafe { Vec::from_raw_parts(self.ptr, self._mem_len, 0) }
	}

	/// Copies the given memory area into a Vector.
	fn cpy_into_vec(&self) -> Vec<u8> {
		let mut vec = vec![0u8; self.len];
		vec.copy_from_slice(self.deref());
		vec
	}

	/// Copies the given memory area into a Box.
	fn cpy_into_box(&self) -> Box<[u8]> {
		let mut vec = vec![0u8; self.len];
		vec.copy_from_slice(self.deref());
		vec.into_boxed_slice()
	}

	/// Returns the raw pointer from where the controlled
	/// memory area starts.
	fn raw_ptr(&self) -> *mut u8 {
		self.ptr
	}

	/// Returns the length of the accessible memory area.
	fn len(&self) -> usize {
		self.len
	}

	/// Returns a "clone" of the Object, which will NOT be deallocated in order
	/// to prevent double frees!
	///
	/// **WARNING**
	///
	/// Be cautious with the usage of clones of `MemDescr`. Typically this function
	/// should only be used to create a second controlling descriptor of an
	/// indirect buffer. See [Buffer] `Buffer::Indirect` for details!
	fn no_dealloc_clone(&self) -> Self {
		MemDescr {
			ptr: self.ptr,
			len: self.len,
			_init_len: self.len(),
			_mem_len: self._mem_len,
			id: None,
			pool: Rc::clone(&self.pool),
			dealloc: Dealloc::Not,
		}
	}
}

impl Deref for MemDescr {
	type Target = [u8];
	fn deref(&self) -> &Self::Target {
		unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
	}
}

impl DerefMut for MemDescr {
	fn deref_mut(&mut self) -> &mut Self::Target {
		unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len) }
	}
}

impl Drop for MemDescr {
	fn drop(&mut self) {
		// Handle returning of Id's to pool
		if let Some(id) = self.id.take() {
			self.pool.ret_id(id);
		}

		match self.dealloc {
			Dealloc::Not => (),
			Dealloc::AsSlice => unsafe { drop(Vec::from_raw_parts(self.ptr, self._mem_len, 0)) },
			Dealloc::AsPage => {
				crate::mm::deallocate(VirtAddr::from(self.ptr as usize), self._mem_len)
			}
		}
	}
}

/// A newtype for descriptor ids, for better readability.
struct MemDescrId(pub u16);

/// A newtype for a usize, which indiactes how many bytes the usize does refer to.
#[derive(Debug, Clone, Copy)]
pub struct Bytes(usize);

// Public interface for Bytes
impl Bytes {
	/// Ensures the provided size is never greater than u32::MAX, as this is the maximum
	/// allowed size in the virtio specification.
	/// Returns a None therefore, if the size was to large.
	pub fn new(size: usize) -> Option<Bytes> {
		if core::mem::size_of_val(&size) <= core::mem::size_of::<u32>() {
			// Usize is as maximum 32bit large. Smaller is not a problem for the queue
			Some(Bytes(size))
		} else if core::mem::size_of_val(&size) == core::mem::size_of::<u64>() {
			// Usize is equal to 64 bit
			if (size as u64) <= (u32::MAX as u64) {
				Some(Bytes(size))
			} else {
				None
			}
		} else {
			// No support for machines over 64bit
			None
		}
	}
}

impl From<Bytes> for usize {
	fn from(byte: Bytes) -> Self {
		byte.0
	}
}

enum Dealloc {
	Not,
	AsPage,
	AsSlice,
}

/// MemPool allows to easily control, request and provide memory for Virtqueues.
///
/// * The struct is initialized with a limit of free running "tracked" (see `fn pull_untracked`)
///   memory descriptors. As Virtqueus do only allow a limited amount of descriptors in their queue,
///   the independent queues, can control the number of descriptors by this.
/// * Furthermore the MemPool struct provides an interface to easily retrieve memory of a wanted size
///   via its `fn pull()`and `fn pull_untracked()` functions.
///   The functions return a (MemDescr)[MemDescr] which provides an interface to read and write memory safely and handles clean up of memory
///   upon being dropped.
///   * `fn pull()`: Pulls a memory descriptor which refers to a memory of a defined size. The descriptor does consume an ID from the pool
///      and hence reduces the amount of left descriptors in the pool. Upon drop this ID will be returned to the pool.
///   * `fn pull_untracked`: Pulls a memory descriptor which refers to a memory of a defined size. The descriptor does NOT consume an ID and
///      hence does not reduce the amount of left descriptors in the pool.
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
		// Not really safe "as usize". But the minimum usize on rust is currently
		// usize = 16bit. So it should work, as long as this does not change changes.
		// Thus asserting here, to catch this change!
		assert!(core::mem::size_of::<usize>() >= 2);

		let mut id_vec = Vec::with_capacity(size as usize);

		for i in 1..(size + 1) {
			id_vec.push(MemDescrId(i));
		}

		MemPool {
			pool: RefCell::new(id_vec),
			limit: size,
		}
	}

	/// Creates a MemDescr which refers to already existing memory.
	///
	/// **Info on Usage:**
	/// * `Panics` if given `slice.len() == 0`
	/// * `Panics` if slice crosses physical page boundary
	/// * The given slice MUST be a heap allocated slice.
	/// * Panics if slice crosses page boundaries!
	///
	/// **Properties of Returned MemDescr:**
	///
	/// * The descriptor will consume one element of the pool.
	/// * The referred to memory area will NOT be deallocated upon drop.
	fn pull_from_raw(self: Rc<Self>, slice: &[u8]) -> Result<MemDescr, VirtqError> {
		// Zero sized descriptors are NOT allowed
		// This also prohibids a panic due to accessing wrong index below
		assert!(!slice.is_empty());

		// Assert descriptor does not cross a page barrier
		let start_virt = ptr::from_ref(slice.first().unwrap()).addr();
		let end_virt = ptr::from_ref(slice.last().unwrap()).addr();
		let end_phy_calc = paging::virt_to_phys(VirtAddr::from(start_virt)) + (slice.len() - 1);
		let end_phy = paging::virt_to_phys(VirtAddr::from(end_virt));

		assert_eq!(end_phy, end_phy_calc);

		let desc_id = match self.pool.borrow_mut().pop() {
			Some(id) => id,
			None => return Err(VirtqError::NoDescrAvail),
		};

		Ok(MemDescr {
			ptr: slice.as_ptr() as *mut _,
			len: slice.len(),
			_init_len: slice.len(),
			_mem_len: slice.len(),
			id: Some(desc_id),
			dealloc: Dealloc::Not,
			pool: self.clone(),
		})
	}

	/// Creates a MemDescr which refers to already existing memory.
	/// The MemDescr does NOT consume a place in the pool and should
	/// be used with `Buffer::Indirect`.
	///
	/// **Info on Usage:**
	/// * `Panics` if given `slice.len() == 0`
	/// * `Panics` if slice crosses physical page boundary
	/// * The given slice MUST be a heap allocated slice.
	///
	/// **Properties of Returned MemDescr:**
	///
	/// * The descriptor will consume one element of the pool.
	/// * The referred to memory area will NOT be deallocated upon drop.
	fn pull_from_raw_untracked(self: Rc<Self>, slice: &[u8]) -> MemDescr {
		// Zero sized descriptors are NOT allowed
		// This also prohibids a panic due to accessing wrong index below
		assert!(!slice.is_empty());

		// Assert descriptor does not cross a page barrier
		let start_virt = ptr::from_ref(slice.first().unwrap()).addr();
		let end_virt = ptr::from_ref(slice.last().unwrap()).addr();
		let end_phy_calc = paging::virt_to_phys(VirtAddr::from(start_virt)) + (slice.len() - 1);
		let end_phy = paging::virt_to_phys(VirtAddr::from(end_virt));

		assert_eq!(end_phy, end_phy_calc);

		MemDescr {
			ptr: slice.as_ptr() as *mut _,
			len: slice.len(),
			_init_len: slice.len(),
			_mem_len: slice.len(),
			id: None,
			dealloc: Dealloc::Not,
			pool: self.clone(),
		}
	}

	/// Pulls a memory descriptor, which owns a memory area of the specified size in bytes. The
	/// descriptor does consume an ID and hence reduces the amount of descriptors left in the pool by one.
	///
	/// **INFO:**
	/// * Fails (returns VirtqError), if the pool is empty.
	/// * ID`s of descriptor are by no means sorted. A descriptor can contain an ID between 1 and size_of_pool.
	/// * Calleys can NOT rely on the next pulled descriptor to contain the subsequent ID after the previously
	///   pulled descriptor.
	///   In essence this means MemDesc can contain arbitrary ID's. E.g.:
	///   * First MemPool.pull -> MemDesc with id = 3
	///   * Second MemPool.pull -> MemDesc with id = 100
	///   * Third MemPool.pull -> MemDesc with id = 2,
	fn pull(self: Rc<Self>, bytes: Bytes) -> Result<MemDescr, VirtqError> {
		let id = match self.pool.borrow_mut().pop() {
			Some(id) => id,
			None => return Err(VirtqError::NoDescrAvail),
		};

		let len = bytes.0;

		// Allocate heap memory via a vec, leak and cast
		let _mem_len = len.align_up(BasePageSize::SIZE as usize);
		let ptr = ptr::with_exposed_provenance_mut(crate::mm::allocate(_mem_len, true).0 as usize);

		// Assert descriptor does not cross a page barrier
		let start_virt = ptr as usize;
		let end_virt = start_virt + (len - 1);
		let end_phy_calc = paging::virt_to_phys(VirtAddr::from(start_virt)) + (len - 1);
		let end_phy = paging::virt_to_phys(VirtAddr::from(end_virt));

		assert_eq!(end_phy, end_phy_calc);

		Ok(MemDescr {
			ptr,
			len,
			_init_len: len,
			_mem_len,
			id: Some(id),
			dealloc: Dealloc::AsPage,
			pool: self.clone(),
		})
	}

	/// Pulls a memory descriptor, which owns a memory area of the specified size in bytes. The
	/// descriptor consumes NO ID and hence DOES NOT reduce the amount of descriptors left in the pool.
	/// * ID`s of descriptor are by no means sorted. A descriptor can contain an ID between 1 and size_of_pool.
	/// * Calleys can NOT rely on the next pulled descriptor to contain the subsequent ID after the previously
	///   pulled descriptor.
	///   In essence this means MemDesc can contain arbitrary ID's. E.g.:
	///   * First MemPool.pull -> MemDesc with id = 3
	///   * Second MemPool.pull -> MemDesc with id = 100
	///   * Third MemPool.pull -> MemDesc with id = 2,
	fn pull_untracked(self: Rc<Self>, bytes: Bytes) -> MemDescr {
		let len = bytes.0;

		// Allocate heap memory via a vec, leak and cast
		let _mem_len = len.align_up(BasePageSize::SIZE as usize);
		let ptr = ptr::with_exposed_provenance_mut(crate::mm::allocate(_mem_len, true).0 as usize);

		// Assert descriptor does not cross a page barrier
		let start_virt = ptr as usize;
		let end_virt = start_virt + (len - 1);
		let end_phy_calc = paging::virt_to_phys(VirtAddr::from(start_virt)) + (len - 1);
		let end_phy = paging::virt_to_phys(VirtAddr::from(end_virt));

		assert_eq!(end_phy, end_phy_calc);

		MemDescr {
			ptr,
			len,
			_init_len: len,
			_mem_len,
			id: None,
			dealloc: Dealloc::AsPage,
			pool: self.clone(),
		}
	}
}

/// Specifies the type of buffer and amount of memory chunks that buffer does consist of wanted.
///
///
/// # Examples
/// ```
/// // Describes a buffer consisting of a single chunk of memory. Buffer is 80 bytes large.
/// // Consumes one place in the virtqueue.
///  let single = BuffSpec::Single(Bytes(80));
///
/// // Describes a buffer consisting of a list of memory chunks.
/// // Each chunk of memory consumes one place in the virtqueue.
/// // Buffer in total is 120 bytes large and consumes 3 virtqueue places.
/// // The first chunk of memory is 20 bytes large, the second is 70 bytes large and the third
/// // is 30 bytes large.
/// let desc_lst = [Bytes(20), Bytes(70), Bytes(30)];
/// let multiple = BuffSpec::Multiple(&desc_lst);
///
/// // Describes a buffer consisting of a list of memory chunks. The only difference between
/// // Indirect and Multiple is, that the Indirect descriptor consumes only a single place
/// // in the virtqueue. This virtqueue entry then refers to a list, which tells the device
/// // where the other memory chunks are located. I.e. where the actual data is and where
/// // the device actually can write to.
/// // Buffer in total is 120 bytes large and consumes 1 virtqueue places.
/// // The first chunk of memory is 20 bytes large, the second is 70 bytes large and the third
/// // is 30 bytes large.
/// let desc_lst = [Bytes(20), Bytes(70), Bytes(30)];
/// let indirect = BuffSpec::Indirect(&desc_lst);
///
/// ```
#[derive(Debug, Clone)]
pub enum BuffSpec<'a> {
	/// Create a buffer with a single descriptor of size `Bytes`
	Single(Bytes),
	/// Create a buffer consisting of multiple descriptors, where each descriptors size
	// is defined by  the respective `Bytes` inside the slice. Overall buffer will be
	// the sum of all `Bytes` in the slide
	Multiple(&'a [Bytes]),
	/// Create a buffer consisting of multiple descriptors, where each descriptors size
	// is defined by  the respective `Bytes` inside the slice. Overall buffer will be
	// the sum of all `Bytes` in the slide. But consumes only ONE descriptor of the actual
	/// virtqueue.
	Indirect(&'a [Bytes]),
}

/// Virtqueue descr flags as defined in the specification.
///
/// See Virtio specification v1.1. - 2.6.5
///                          v1.1. - 2.7.1
///
/// INFO: `VIRQ_DESC_F_AVAIL` and `VIRTQ_DESC_F_USED` are only valid for packed
/// virtqueues.
#[allow(dead_code, non_camel_case_types)]
#[derive(Debug, Copy, Clone)]
#[repr(u16)]
pub enum DescrFlags {
	VIRTQ_DESC_F_NEXT = 1 << 0,
	VIRTQ_DESC_F_WRITE = 1 << 1,
	VIRTQ_DESC_F_INDIRECT = 1 << 2,
	VIRTQ_DESC_F_AVAIL = 1 << 7,
	VIRTQ_DESC_F_USED = 1 << 15,
}
use core::ops::Not;
impl Not for DescrFlags {
	type Output = u16;

	fn not(self) -> Self::Output {
		!(u16::from(self))
	}
}

use core::ops::BitOr;
impl BitOr for DescrFlags {
	type Output = u16;
	fn bitor(self, rhs: DescrFlags) -> Self::Output {
		u16::from(self) | u16::from(rhs)
	}
}

impl BitOr<DescrFlags> for u16 {
	type Output = u16;
	fn bitor(self, rhs: DescrFlags) -> Self::Output {
		self | u16::from(rhs)
	}
}

impl BitAnd for DescrFlags {
	type Output = u16;

	fn bitand(self, rhs: Self) -> Self::Output {
		u16::from(self) & u16::from(rhs)
	}
}

impl BitAnd<DescrFlags> for u16 {
	type Output = u16;

	fn bitand(self, rhs: DescrFlags) -> Self::Output {
		self & u16::from(rhs)
	}
}

impl PartialEq<DescrFlags> for u16 {
	fn eq(&self, other: &DescrFlags) -> bool {
		*self == u16::from(*other)
	}
}

impl From<DescrFlags> for u16 {
	fn from(flag: DescrFlags) -> Self {
		match flag {
			DescrFlags::VIRTQ_DESC_F_NEXT => 1 << 0,
			DescrFlags::VIRTQ_DESC_F_WRITE => 1 << 1,
			DescrFlags::VIRTQ_DESC_F_INDIRECT => 1 << 2,
			DescrFlags::VIRTQ_DESC_F_AVAIL => 1 << 7,
			DescrFlags::VIRTQ_DESC_F_USED => 1 << 15,
		}
	}
}

/// Virtqeueus error module.
///
/// This module unifies errors provided to useres of a virtqueue, independent of the underlying
/// virtqueue implementation, realized via the different enum variants.
pub mod error {
	use crate::fd;

	#[derive(Debug)]
	// Internal Error Handling for Buffers
	pub enum BufferError {
		WriteToLarge,
		ToManyWrites,
	}

	// External Error Handling for users of the virtqueue.
	pub enum VirtqError {
		General,
		/// Indirect is mixed with Direct descriptors, which is not allowed
		/// according to the specification.
		/// See [Buffer](super::Buffer) and [BuffSpec](super::BuffSpec) for details
		BufferInWithDirect,
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
		/// Indicates that a [BuffSpec](super::BuffSpec) does have the right size
		/// for a given structure. Returns the structures size in bytes.
		///
		/// E.g: A struct `T` with size of `4 bytes` must have a `BuffSpec`, which
		/// defines exactly 4 bytes. Regardeless of whether it is a `Single`, `Multiple`
		/// or `Indirect` BuffSpec.
		BufferSizeWrong(usize),
		/// The requested BufferToken for reuse is signed as not reusable and hence
		/// can not be used twice.
		/// Typically this is the case if one created the BufferToken indirectly
		/// via `Virtq.prep_transfer_from_raw()`. Due to the fact, that reusing
		/// Buffers which refer to raw pointers seems dangerours, this is forbidden.
		NoReuseBuffer,
		/// Indicates a write into a Buffer that is not existing
		NoBufferAvail,
		/// Indicates that a write to a Buffer happened and the data to be written into
		/// the buffer/descriptor was to large for the buffer.
		WriteTooLarge,
		/// Indicates that a Bytes::new() call failed or generally that a buffer is to large to
		/// be transferred as one. The Maximum size is u32::MAX. This also is the maximum for indirect
		/// descriptors (both the one placed in the queue, as also the ones the indirect descriptor is
		/// referring to).
		BufferToLarge,
		QueueSizeNotAllowed(u16),
		FeatureNotSupported(virtio_spec::F),
		AllocationError,
	}

	impl core::fmt::Debug for VirtqError {
		fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
			match self {
                VirtqError::General => write!(f, "Virtq failure due to unknown reasons!"),
                VirtqError::NoBufferAvail => write!(f, "Virtq detected write into non existing Buffer!"),
                VirtqError::BufferInWithDirect => write!(f, "Virtq detected creation of Token, where Indirect and direct buffers where mixed!"),
                VirtqError::BufferNotSpecified => write!(f, "Virtq detected creation of Token, without a BuffSpec"),
                VirtqError::QueueNotExisting(_) => write!(f, "Virtq does not exist and can not be used!"),
                VirtqError::NoDescrAvail => write!(f, "Virtqs memory pool is exhausted!"),
                VirtqError::BufferSizeWrong(_) => write!(f, "Specified Buffer is to small for write!"),
                VirtqError::NoReuseBuffer => write!(f, "Buffer can not be reused!"),
                VirtqError::WriteTooLarge => write!(f, "Write is to large for BufferToken!"),
                VirtqError::BufferToLarge => write!(f, "Buffer to large for queue! u32::MAX exceeded."),
				VirtqError::QueueSizeNotAllowed(_) => write!(f, "The requested queue size is not valid."),
				VirtqError::FeatureNotSupported(_) => write!(f, "An unsupported feature was requested from the queue."),
				VirtqError::AllocationError => write!(f, "An error was encountered during the allocation of the queue structures.")
            }
		}
	}

	impl core::convert::From<VirtqError> for fd::IoError {
		fn from(_: VirtqError) -> Self {
			fd::IoError::EIO
		}
	}
}
