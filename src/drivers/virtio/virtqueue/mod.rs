// Copyright (c) 2020 Frederik Schulz, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! This module contains Virtio's virtqueue. 
//!
//! The virtqueue is available in two forms. 
//! [Split](structs.SplitVqueue.html) and [Packed](structs.PackedVqueue.html).
//! Both queues are wrapped inside an enum [Virtqueue](enums.Virtqueue.html) in
//! order to provide an unified interface. 
//! 
//! Drivers who need a more fine grained access to the specifc queues must
//! use the respective virtqueue structs directly.
pub mod packed;
pub mod split;

use self::packed::PackedVq;
use self::split::SplitVq;
use self::error::VirtqError;

use super::transport::pci::ComCfg;
use alloc::vec::Vec;
use alloc::boxed::Box;

use alloc::collections::VecDeque;
use core::ops::{BitAnd, Deref, DerefMut};
use core::cell::RefCell;
use alloc::rc::Rc;

/// A u16 newtype. If instantiated via ``VqIndex::from(T)``, the newtype is ensured to be
/// smaller-equal to `min(u16::MAX , T::MAX)`. 
///
/// Currently implements `From<u16>` and `From<u32>`.
#[derive(Copy, Clone, Debug, PartialOrd, PartialEq)]
pub struct VqIndex(u16);

impl From<u16> for VqIndex {
    fn from(val: u16) -> Self {
        VqIndex(val)
    }
}

impl Into<u16> for VqIndex {
    fn into(self) -> u16 {
        self.0
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
#[derive(Copy, Clone, Debug, PartialOrd, PartialEq)]
pub struct VqSize(u16);

impl From<u16> for VqSize {
    fn from(val: u16) -> Self {
        VqSize(val)
    }
}

impl From<u32> for VqSize{
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

/// Enum that defines which virtqueue shall be created when used via the `Virtq::new()` function.
pub enum VqType{
    Packed,
    Split,
}

/// The Virtq enum unifies access to the two different Virtqueue types 
/// [PackedVq](structs.PackedVq.html) and [SplitVq](structs.SplitVq.html).
///
/// The enum provides a common interface for both types. Which in some case
/// might not provide the complete feature set of each queue. Drivers who
/// do need these features should refrain from providing support for both 
/// Virtqueue types and use the structs directly instead.
pub enum Virtq {
    Packed(PackedVq),
    Split(SplitVq),
}

// Public interface of Virtq
impl Virtq {
    /// Dispatches a batch of TransferTokens. The actuall behaviour depends on the respective 
    /// virtqueue implementation. Pleace see the respective docs for details
    pub fn dispatch_batch(tkns: Vec<TransferToken>) -> Vec<Transfer> {
        todo!("Implement dispatch, best would be with a HashMap(index, Vec<Tkn>)");
    }

    /// Dispatches a batch of TransferTokens. The Transfers will be placed in to the `await_queue`
    /// upon finish.
    ///
    /// The actuall behaviour depends on the respective 
    /// virtqueue implementation. Please see the respective docs for details.
    pub fn dispatch_batch_await(tkns: Vec<TransferToken>, await_queue: Rc<RefCell<VecDeque<Transfer>>>) {
        todo!("Implement dispatch, best would be with a HashMap(index, Vec<Tkn>)");
    }

    pub fn dispatch(&self, tkn: TransferToken) -> Transfer {
        match self {
            Virtq::Packed(vq) => vq.dispatch(tkn),
            Virtq::Split(vq) => unimplemented!(),
        }
    }
    
    // Creates a new Virtq of the specified (VqType)[VqType], (VqSize)[VqSize] and the (VqIndex)[VqIndex]. 
    /// The index represents the "ID" of the virtqueue. 
    /// Upon creation the virtqueue is "registered" at the device via the `ComCfg` struct.
    ///
    /// Be aware, that devices define a maximum number of queues and a maximal size they can handle.
    pub fn new(com_cfg: &mut ComCfg, size: VqSize, vq_type: VqType, index: VqIndex) -> Self {
        match vq_type {
            VqType::Packed => match PackedVq::new(com_cfg, size, index) {
                Ok(packed_vq) => Virtq::Packed(packed_vq),
                Err(vq_error) => panic!("Currently panics if queue fails to be created")
            },
            VqType::Split => unimplemented!()
        }
    }

    /// Returns the size of a Virtqueue. This represents the overall size and not the capacity the
    /// queue currently has for new descriptors.
    pub fn size(&self) -> VqSize {
        match self {
            Virtq::Packed(vq) => vq.size(),
            Virtq::Split(vq) => vq.size(),
        }
    }

    // Returns the index (ID) of a Virtqueue. 
    pub fn index(&self) -> VqIndex {
        match self {
            Virtq::Packed(vq) => vq.index(),
            Virtq::Split(vq) => unimplemented!(),
        }
    } 

    /// Provides the calley with a TransferToken. Fails upon multiple circumstances.
    /// 
    /// **Parameters**
    /// * send: `Option<(Box<T>, BuffSpec)>`
    ///     * None: No send buffers are provided to the device
    ///     * Some: 
    ///         * `T` defines the structure which will be provided to the device
    ///         * [BuffSpec](BuffSpec) defines how this struct will be presented to the device. 
    ///         See documentation on `BuffSpec` for details.
    /// * recv: `Option<(Box<K>, BuffSpec)>`
    ///     * None: No buffers, which are writable for the device are provided to the device.
    ///     * Some: 
    ///         * `K` defines the structure which will be provided to the device
    ///         * [BuffSpec](BuffSpec) defines how this struct will be presented to the device. 
    ///         See documentation on `BuffSpec` for details.
    ///
    /// **Reasons for Failure:**
    /// * Queue does not have enough descriptors left, to split `T` or `K` into the desired amount of memory chunks.
    /// * Calley mixed `Indirect (Direct::Indirect())` with `Direct(BuffSpec::Single() or BuffSpec::Multiple())` descriptors.
    ///
    /// **Details on Usage:**
    ///
    /// * Calley is not allowed to mix `Indirect` and `Direct` descriptors. Furthermore if the calley decides to use `Indirect`
    /// descriptors, the queue will merge the send and recv structure as follows:
    /// ```
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
    /// Then he must split the strucutre after the send part and provide the respective part via the send argument and the respective other 
    /// part via the recv argument.
    pub fn prep_transfer<T: AsSliceU8 + 'static, K: AsSliceU8 + 'static>(&self, rc_self: Rc<Virtq>, send: Option<(Box<T>, BuffSpec)>, recv: Option<(Box<K>, BuffSpec)>) -> Result<TransferToken, VirtqError> {
            match self {
                Virtq::Packed(vq) => vq.prep_transfer(rc_self ,send, recv),
                Virtq::Split(vq ) => unimplemented!(),
            }
    }

    /// Provides the calley with a TransferToken. Fails upon multiple circumstances.
    ///
    /// **INFO:**
    /// * Data behind the respective raw pointers will NOT be deallocated. Under no circumstances. 
    /// * Calley is responsible for ensuring the raw pointers will remain valid from start till end of transfer.
    ///   * start: call of `fn prep_transfer_from_raw()`
    ///   * end: closing of [Transfer](Transfer) via `Transfer.close()`.
    ///   * In case the underlying BufferToken is reused, the raw pointers MUST still be valid all the time
    ///   BufferToken exists.
    /// * Transfer created from this TransferTokens will ONLY allow to return a copy of the data.
    ///   * This is due to the fact, that the `Transfer.ret()` returns a `Box[u8]`, which must own
    ///   the array. This would lead to unwanted frees, if not handled carefully
    /// * Drivers must take care of keeping a copy of the respective `*mut T` and `*mut K` for themselves
    /// 
    /// **Parameters**
    /// * send: `Option<(*mut T, BuffSpec)>`
    ///     * None: No send buffers are provided to the device
    ///     * Some: 
    ///         * `T` defines the structure which will be provided to the device
    ///         * [BuffSpec](BuffSpec) defines how this struct will be presented to the device. 
    ///         See documentation on `BuffSpec` for details.
    /// * recv: `Option<(*mut K, BuffSpec)>`
    ///     * None: No buffers, which are writable for the device are provided to the device.
    ///     * Some: 
    ///         * `K` defines the structure which will be provided to the device
    ///         * [BuffSpec](BuffSpec) defines how this struct will be presented to the device. 
    ///         See documentation on `BuffSpec` for details.
    ///
    /// **Reasons for Failure:**
    /// * Queue does not have enough descriptors left, to split `T` or `K` into the desired amount of memory chunks.
    /// * Calley mixed `Indirect (Direct::Indirect())` with `Direct(BuffSpec::Single() or BuffSpec::Multiple())` descriptors.
    ///
    /// **Details on Usage:**
    ///
    /// * Calley is not allowed to mix `Indirect` and `Direct` descriptors. Furthermore if the calley decides to use `Indirect`
    /// descriptors, the queue will merge the send and recv structure as follows:
    /// ```
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
    /// Then he must split the strucutre after the send part and provide the respective part via the send argument and the respective other 
    /// part via the recv argument. 
    pub fn prep_transfer_from_raw<T: AsSliceU8 + 'static, K: AsSliceU8 + 'static>(&self, rc_self: Rc<Virtq>, send: Option<(*mut T, BuffSpec)>, recv: Option<(*mut K, BuffSpec)>) -> Result<TransferToken, VirtqError> {
        match self {
            Virtq::Packed(vq) => vq.prep_transfer_from_raw(rc_self, send, recv),
            Virtq::Split(vq ) => unimplemented!(),
        }
    }

    /// Provides the calley with empty buffers as specified via the `send` and `recv` function parameters, (see [BuffSpec](BuffSpec)), in form of 
    /// a [BufferToken](BufferToken).
    /// Fails upon multiple circumstances.
    ///
    /// **Parameters**
    /// * send: `Option<BuffSpec>`
    ///     * None: No send buffers are provided to the device
    ///     * Some: 
    ///         * [BuffSpec](BuffSpec) defines the size of the buffer and how the buffer is 
    ///         Buffer will be structured. See documentation on `BuffSpec` for details.
    /// * recv: `Option<BuffSpec>`
    ///     * None: No buffers, which are writable for the device are provided to the device.
    ///     * Some: 
    ///         * [BuffSpec](BuffSpec) defines the size of the buffer and how the buffer is 
    ///         Buffer will be structured. See documentation on `BuffSpec` for details.
    ///
    /// ** Reasons for Failure:** 
    /// * Queue does not have enough descriptors left to create the desired amount of descriptors as indicated by the `BuffSpec`.
    /// * Calley mixed `Indirect (Direct::Indirect())` with `Direct(BuffSpec::Single() or BuffSpec::Multiple())` descriptors.
    /// * Systerm does not have enough memory resources left.
    /// 
    /// **Details on Usage:**
    ///
    /// * Calley is not allowed to mix `Indirect` and `Direct` descriptors. Furthermore if the calley decides to use `Indirect`
    /// descriptors, the queue will merge the send and recv structure as follows:
    /// ```
    /// //+++++++++++++++++++++++
    /// //+        Queue        +
    /// //+++++++++++++++++++++++
    /// //+ Indirect descriptor + -> refers to a descriptor list in the form of ->  ++++++++++++++++++++++++++
    /// //+         ...         +                                                   +     Descriptors for T  +
    /// //+++++++++++++++++++++++                                                   +     Descriptors for K  +
    /// //                                                                          ++++++++++++++++++++++++++
    /// ```
    /// As a result indirect descriptors result in a single descriptor consumption in the actual queue.
    pub fn prep_buffer(&self, rc_self: Rc<Virtq>, send: Option<BuffSpec>, recv: Option<BuffSpec>) -> Result<BufferToken, VirtqError> {
        match self {
            Virtq::Packed(vq) => vq.prep_buffer(rc_self, send, recv),
            Virtq::Split(vq ) => unimplemented!(),
        }
    }

    /// Early drop provides a mechanism for the queue to detect, if an ongoing transfer or a transfer not yet polled by the driver 
    /// has been dropped. The queue implementation is responsible for taking care what should happen to the respective TransferToken
    /// and BufferToken.
    fn early_drop(&self, transfer_tk: Pinned<TransferToken>) {
        match self {
            Virtq::Packed(vq) => vq.early_drop(transfer_tk),
            Virtq::Split(vq ) => unimplemented!(),
        }
    }
}

/// The trait needs to be implemented on structures which are to be used via the `prep_transfer()` function of virtqueues and for 
/// structures which are to be used to write data into buffers of a [BufferToken](BufferToken) via `BufferToken.write()` or
/// `BufferToken.write_seq()`.
///
/// **INFO:*
/// The trait provides a decent default implementation. Please look at the code for details. 
/// The provided default implementation computes the size of the given structure via `core::mem::size_of_val(&self)`
/// and then casts the given `*const Self` pointer of the structure into an `*const u8`. 
/// 
/// Users must be really carefull, and check, wether the memory representation of the given structure equals
/// the representation the device expects. It is advised to only use `#[repr(C)]` and to check the output 
/// of `as_slice_u8` and `as_slice_u8_mut`.
pub trait AsSliceU8 {
    /// Returns a slice of the given structure.
    ///
    /// ** WARN:**
    /// * The slice must be little endian coded in order to be understood by the device
    /// * The slice must serialize the actual structure the device expects, as the queue will use 
    /// the addresses of the slice in order to refer to the structure.
    fn as_slice_u8(&self) -> &[u8] {
        unsafe { 
            core::slice::from_raw_parts(
            (self as *const Self) as *const u8,
                core::mem::size_of_val(&self),
            )
        }
    }

    /// Returns a mutable slice of the given structure.
    ///
    /// ** WARN:**
    /// * The slice must be little endian coded in order to be understood by the device
    /// * The slice must serialize the actual structure the device expects, as the queue will use 
    /// the addresses of the slice in order to refer to the structure.
     fn as_slice_u8_mut(&self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(
                (self as *const Self) as *mut u8,
                core::mem::size_of_val(&self)
            )
        }
    }
}


/// The [Transfer](Transfer) will be received when a [TransferToken](TransferToken) is dispatched via `TransferToken.dispatch()` or 
/// via `TransferToken.dispatch_blocking()`.
///
/// The struct represents an ongoing transfer or an active transfer. While this does NOT mean, that the transfer is at all times inside
/// actual virtqueue. The Transfers state can be polled via `Transfer.poll()`, which returns a bool if the transfer is finished.
/// 
/// **Finished Transfers:**
/// * Finished transfers are able to return their send and receive buffers. Either as a copy via `Transfer.ret_cpy()` or as the actual 
/// buffers via `Transfer.ret()`.
/// * Finished transfers should be closed via `Transfer.close()` or via `Transfer.ret()`.
/// * Finished transfers can be reused via `Transfer.reuse()`.
///   * This closes the transfer
///   * And returns an normal BufferToken (One should be cautious with reusing transfers where buffers were created from raw pointers)
///
/// **Early dropped Transfers:**
///
/// If a transfer is dropped without beeing closed (independent of beeing finished or ongoing), the transfer will return the respective
/// `Pinned<TransferToken>` to the handling virtqueue, which will take of handling gracefull shutdown. Which generally should take 
/// care of waiting till the device handled the respective transfer and free the memory afterwards. 
///
/// One could "abuse" this procedure in order to realize a "fire-and-forget" transfer. 
/// A warning here: The respective queue implementation is taking care of handling this case and there are no guarantees that the queue 
/// won't be unusable afterwards. 
pub struct Transfer {
    /// Needs to be Option<Pinned<TransferToken>> in order to prevent deallocation via None
    // See custom drop function for clarity
    transfer_tkn: Option<Pinned<TransferToken>>, 
    await_queue: Option<Rc<RefCell<VecDeque<Transfer>>>>,
}

impl Drop for Transfer {
    /// When an unclosed transfer is dropped. The [Pinned](Pinned)<[TransferToken](struct.TransferToken.html)> is returned to the respective
    /// virtqueue, who is responsible of handling these situations. 
    fn drop(&mut self) {
        if let Some(tkn) = self.transfer_tkn.take() {
            // Unwrapping is okay here, as TransferToken MUST hold a BufferToken
            let vq_ref = Rc::clone(&tkn.buff_tkn.as_ref().unwrap().vq);
            vq_ref.early_drop(tkn)
        }
    }
}

// Public Interface of Transfer
impl Transfer {
    /// Used to poll the current state of the transfer.
    /// * true = Transfer is finished and can be closed, reused or return data
    /// * false = Transfer is ongoing
    pub fn poll(&self)-> bool {
        // Unwrapping is okay here, as Transfers must hold a TransferToken
        match self.transfer_tkn.as_ref().unwrap().state {
            TransferState::Finished => true,
            TransferState::Ready => unreachable!("Transfers owned by other than queue should have Tokens, of Finished or Processing State!"),
            TransferState::Processing => false,
        }
    }

    /// Returns a copy if the respective send and receiving buffers
    /// The actul buffers remain in the BufferToken and hence the token can be 
    /// reused afterwards.
    /// 
   /// **Return Tuple**
    ///
    /// `(sended_data, received_data)`
    /// 
    /// The returned data is of type `Box<[Box<[u8]>]>`. This function therefore preserves
    /// the scattered structure of the buffer,
    /// 
    /// If one create this buffer via a `Virtq.prep_transfer()` or `Virtq.prep_transfer_from_raw()` 
    /// call, a casting back to the original structure `T` is NOT possible.
    /// In theses cases please use `Transfer.ret_cpy()` or use 'BuffSpec::Single' only!
    pub fn ret_scat_cpy(&self) -> Result<(Option<Box<[Box<[u8]>]>>, Option<Box<[Box<[u8]>]>>), VirtqError> {
        match &self.transfer_tkn.as_ref().unwrap().state {
            TransferState::Finished => {
                // Unwrapping is okay here, as TransferToken must hold a BufferToken
                let send_data = match &self.transfer_tkn.as_ref().unwrap().buff_tkn.as_ref().unwrap().send_buff {
                    Some(buff) => Some(buff.scat_cpy()),
                    None => None,
                };

                let recv_data = match &self.transfer_tkn.as_ref().unwrap().buff_tkn.as_ref().unwrap().send_buff {
                    Some(buff) => Some(buff.scat_cpy()),
                    None => None,
                };

                Ok((send_data, recv_data))
            },
            TransferState::Processing => Err(VirtqError::OngoingTransfer(None)),
            TransferState::Ready => unreachable!("Transfers not owned by a queue Must have state Finished or Processing!"), 
        }
    }

    /// Returns a copy if the respective send and receiving buffers
    /// The actul buffers remain in the BufferToken and hence the token can be 
    /// reused afterwards.
    /// 
    /// **Return Tuple**
    ///
    /// `(sended_data, received_data)`
    /// 
    /// The sended_data is `Box<[u8]>`. This function herefore merges (if multiple descriptors
    /// were requested for one buffer) into a single `[u8]`. 
    /// 
    /// It can be assumed, that if one created the send buffer from a structure `T`, that 
    /// `&sended_data[0] as *const u8 == *const T`
    pub fn ret_cpy(&self) -> Result<(Option<Box<[u8]>>, Option<Box<[u8]>>), VirtqError> {
        match &self.transfer_tkn.as_ref().unwrap().state {
            TransferState::Finished => {
                // Unwrapping is okay here, as TransferToken must hold a BufferToken
                let send_data = match &self.transfer_tkn.as_ref().unwrap().buff_tkn.as_ref().unwrap().send_buff {
                    Some(buff) => Some(buff.cpy()),
                    None => None,
                };

                let recv_data = match &self.transfer_tkn.as_ref().unwrap().buff_tkn.as_ref().unwrap().send_buff {
                    Some(buff) => Some(buff.cpy()),
                    None => None,
                };

                Ok((send_data, recv_data))
            },
            TransferState::Processing => Err(VirtqError::OngoingTransfer(None)),
            TransferState::Ready => unreachable!("Transfers not owned by a queue Must have state Finished or Processing!"), 
        }
    }

    /// Returns the actual send and receiving buffers.
    /// The function consumes the tranfer and cleans up all tokens.
    /// Failes if `TransferState != Finished`.
    /// 
    /// **Return Tuple**
    ///
    /// `(sended_data, received_data)`
    ///
    /// Returned data is of type `Box<[Box<[u8]>]>` in order to preserve the memory regions
    /// and to prevent copying of data.
    pub fn ret(mut self) -> Result<(Option<Box<[Box<[u8]>]>>, Option<Box<[Box<[u8]>]>>), VirtqError> {
        let state = self.transfer_tkn.as_ref().unwrap().state;

        match state {
            TransferState::Finished => {
                // Desctructure Token
                let mut transfer_tkn = self.transfer_tkn.take().unwrap().into_inner();
                let mut buffer_tkn = transfer_tkn.buff_tkn.take().unwrap();

                let send_data = match buffer_tkn.ret_send {
                    True => match buffer_tkn.send_buff {
                        Some(buff) => {
                            // This data is not a second time returnable
                            // Unessecary, because token will be dropped.
                            // But to be consistent in state.
                            buffer_tkn.ret_send = false;
                            Some(buff.into_boxed())
                        },
                        None => None,
                    },
                    False => None,
                };

                let recv_data = match buffer_tkn.ret_recv {
                    True => match buffer_tkn.recv_buff {
                        Some(buff) => {
                            // This data is not a second time returnable
                            // Unessecary, because token will be dropped.
                            // But to be consistent in state.
                            buffer_tkn.ret_recv = false;
                            Some(buff.into_boxed())
                        },
                        None => None,
                    },
                    False => None,
                };
                // Prevent Token to be reusable although it will be dropped
                // later in this function.
                // Unessecary but to be consistent in state.
                //
                // Unwrapping is okay here, as TransferToken must hold a BufferToken
                buffer_tkn.reusable = false;

                Ok((send_data, recv_data))
            }, 
            TransferState::Processing => Err(VirtqError::OngoingTransfer(Some(self))),
            TransferState::Ready => unreachable!("Transfers not owned by a queue Must have state Finished or Processing!"), 
        }
    }

    /// Closes an transfer. If the transfer was ongoing the respective transfer token will be returned to the virtqueue.
    /// If it was finished the resources will be cleaned up.
    pub fn close(mut self) {
        match self.transfer_tkn.as_ref().unwrap().state {
            TransferState::Processing => {
                // Unwrapping is okay here, as TransferToken must hold a BufferToken
                let vq = Rc::clone(&self.transfer_tkn.as_ref().unwrap().get_vq());
                let transfer_tkn = self.transfer_tkn.take().unwrap();
                vq.early_drop(transfer_tkn);
            },
            TransferState::Ready => unreachable!("Transfers MUST have tokens of states Processing or Finished."),
            TransferState::Finished => (),// Do nothing and free everything.
        }
    }

    /// If the transfer was finished returns the BufferToken inside the transfer else returns an error.
    pub fn reuse(mut self) -> Result<BufferToken, VirtqError> {
        // Unwrapping is okay here, as TransferToken must hold a BufferToken
        match self.transfer_tkn.as_ref().unwrap().state {
            TransferState::Finished => {
                if self.transfer_tkn.as_ref().unwrap().buff_tkn.as_ref().unwrap().reusable {
                    Ok(self.transfer_tkn.take().unwrap().into_inner().buff_tkn.take().unwrap())
                } else {
                    Err(VirtqError::NoReuseBuffer)
                }
            },
            TransferState::Processing => Err(VirtqError::OngoingTransfer(Some(self))),
            TransferState::Ready => unreachable!("Transfers coming from outside the queue must be Processing or Finished"),
        }
    }
}

/// Enum indicates the current state of a transfer.
#[derive(PartialEq, Copy, Clone, Debug)]
enum TransferState {
    /// Queue finished transfer
    Finished,
    /// Transfer is ongoing and still processed by queue
    Processing, 
    /// Transfer is ready to be sended
    Ready,
}

/// The struct represents buffers which are ready to be send via the 
/// virtqueue. Buffers can no longer be written or retrieved. 
pub struct TransferToken {
    state: TransferState,
    /// Must be some in order to prevent drop
    /// upon reuse.
    buff_tkn: Option<BufferToken>,
    /// Structure which allows to await Transfers
    /// If Some, finished TransferTokens will be placed here 
    /// as finished `Transfers`. If None, only the state 
    /// of the Token will be changed.
    await_queue: Option<Rc<RefCell<VecDeque<Transfer>>>>,
}

/// Public Interface for TransferToken
impl TransferToken {
    /// Returns a refernce to the holding virtqueue
    pub fn get_vq(&self) -> Rc<Virtq> {
        // Unwrapping is okay here, as TransferToken must hold a BufferToken
        Rc::clone(&self.buff_tkn.as_ref().unwrap().vq)
    }

    pub fn dispatch_await(mut self, await_queue: Rc<RefCell<VecDeque<Transfer>>>) {
        self.await_queue = Some(Rc::clone(&await_queue));
        
        // Prevent TransferToken from beeing dropped 
        // I.e. do NOT run the costum constructor which will 
        // deallocate memory, as we never call drop upon
        // the ManuallyDrop<Pinned<TransferToken>>
        core::mem::ManuallyDrop::new(self.get_vq().dispatch(self).transfer_tkn.take());
    }

    /// Dispatches the provided TransferToken to the respective queue and returns a transfer.
    pub fn dispatch(self) -> Transfer {
        self.get_vq().dispatch(self)
    }

    /// Dispatches the provided TransferToken to the respectuve queue and does 
    /// return when, the queue finished the transfer.
    ///
    /// The resultaing [TransferState](TransferState) in this case is of course 
    /// finished and the returned [Transfer](Transfer) can be reused, copyied from
    /// or retrun the underlying buffers.
    /// Allthough it is recomended to ensure the finished state via`Transfer.poll()` beforehand.
    pub fn dispatch_blocking(self) -> Result<Transfer, VirtqError> {
        let transfer = self.get_vq().dispatch(self);

        while transfer.transfer_tkn.as_ref().unwrap().state != TransferState::Finished {
            // Keep Spinning untill the state changes to Finished
        }

        Ok(transfer)
    }
}

/// The struct represents buffers which are ready to be writen or to be send.
/// 
/// BufferTokens can be writen in two ways:
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

    vq: Rc<Virtq>,
    /// Indicates wether the buff is returnable
    ret_send: bool,
    ret_recv: bool,
    /// Indicates if the token is allowed 
    /// to be reused.
    reusable: bool,
}

// Public interface of BufferToken
impl BufferToken {
    /// Writes the provided datastructures into the respective buffers. `K` into `self.send_buff` and `H` into 
    /// `self.recv_buff`. 
    /// If the provided datastructures do not "fit" into the respective buffers, the function will return an error. Even
    /// if only one of the two structures is to large.
    /// The same error will be triggered in case the respective buffer wasn't even existing, as not all transfers consist
    /// of send and recv buffers.
    ///
    /// # Detailed Description
    /// The respective send and recv buffers (see [BufferToken](BufferToken) docs for details on buffers) consist of multiple 
    /// descriptors. 
    /// The `write()` function does NOT take into account the distinct descriptors of a buffer but treats the buffer as a sinlge continous 
    /// memeory element and as a result writes `T` or `H` as a slice of bytes into this memory.
    pub fn write<K: AsSliceU8, H: AsSliceU8>(mut self, send: Option<K>, recv: Option<H>) -> Result<TransferToken, VirtqError> {
        unimplemented!();
        // writes K into the send_buff
        // VIrtqError::WriteToLarge(BufferToken) will return the token for recovery
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
    pub fn write_seq<K: AsSliceU8, H: AsSliceU8>(mut self, send_seq: Option<K>, recv_seq: Option<H>) -> Result<Self, VirtqError> {
        todo!("Implement, but need to change PACKED vq, as state is needed in Buffer enum!");
        // If this works and Buffer::{...} can be adjusted accordingly, then must set 
        // last_write to zero, when BuffToken Transformed into TransferToken
        // writes K into the first send_buff element. Next write will be written to the next buff.
    }

    /// Consumes the [BufferToken](BufferToken) and returns a [TransferToken](TransferToken), that can be used to actually start the transfer.
    /// 
    /// After this call, the buffers are no longer writable. 
    pub fn provide(mut self) -> TransferToken {
        todo!("Set Last write to zero! for bufferToken");
    }
}

/// Describes the type of a buffer and unifies them.
enum Buffer {
    /// A buffer consisting of a single [Memory Descriptor](MemDescr).
    Single {
        desc_lst: Box<[MemDescr]>,
        len: usize,
        last_write: usize
    },
    /// A buffer consisting of a chain of [Memory Descriptors](MemDescr). 
    /// Especially useful if one wants to send multiple structures to a device,
    /// as he can sequentially write (see [BufferToken](BufferToken) `write_seq()`)
    /// those structs into the descriptors.
    Multiple {
        desc_lst: Box<[MemDescr]>,
        len: usize,
        last_write: usize
    },
    /// A buffer consisting of a single descriptor in the actuall virtqueue,
    /// referencing a list of descriptors somewhere in memory. 
    /// Especially useful of one wants to extend the capacity of the virtqueue.
    /// Also has the same advantages as a `Buffer::Multiple`.
    Indirect {
        desc_lst: Box<[MemDescr]>,
        ctrl_desc: MemDescr,
        len: usize,
        last_write: usize
    },
}

// Private Interface of Buffer
impl Buffer {
    fn into_boxed(mut self) -> Box<[Box<[u8]>]> {
        match self {
            Buffer::Single{mut desc_lst, last_write, len} => {
                let mut arr = Vec::with_capacity(desc_lst.len());
                
                for desc in desc_lst.iter_mut() {
                    // Need to be a little carefull here. 
                    // As it is NOT possible to move out of Box<[MemDescr]>, we
                    // copy a no_dealloc_clone which is consumed by into_boxed()
                    // and set the actual descriptor.dealloc = false to prevent double frees.
                    desc.dealloc = false;
                    arr.push(desc.no_dealloc_clone().into_boxed());
                }
                arr.into_boxed_slice()
            } ,
            Buffer::Multiple{mut desc_lst, last_write, len} => {
                let mut arr = Vec::with_capacity(desc_lst.len());
                
                for desc in desc_lst.iter_mut() {
                    // Need to be a little carefull here. 
                    // As it is NOT possible to move out of Box<[MemDescr]>, we
                    // copy a no_dealloc_clone which is consumed by into_boxed()
                    // and set the actual descriptor.dealloc = false to prevent double frees.
                    desc.dealloc = false;
                    arr.push(desc.no_dealloc_clone().into_boxed());
                }
                arr.into_boxed_slice()
            } ,
            Buffer::Indirect{mut desc_lst, ctrl_desc, last_write, len} => {
                let mut arr = Vec::with_capacity(desc_lst.len());
                
                for desc in desc_lst.iter_mut() {
                    // Need to be a little carefull here. 
                    // As it is NOT possible to move out of Box<[MemDescr]>, we
                    // copy a no_dealloc_clone which is consumed by into_boxed()
                    // and set the actual descriptor.dealloc = false to prevent double frees.
                    desc.dealloc = false;
                    arr.push(desc.no_dealloc_clone().into_boxed());
                }
                arr.into_boxed_slice()
            } ,
        }
    }

    fn cpy(&self) -> Box<[u8]>{
        match &self {
            Buffer::Single{desc_lst, last_write, len} => {
                let mut arr = Vec::with_capacity(*len);
                
                for desc in desc_lst.iter() {
                    arr.append(&mut desc.cpy_into_vec());
                }
                arr.into_boxed_slice()
            } ,
            Buffer::Multiple{desc_lst, last_write, len} => {
                let mut arr = Vec::with_capacity(*len);
                
                for desc in desc_lst.iter() {
                    arr.append(&mut desc.cpy_into_vec());
                }
                arr.into_boxed_slice()
            } ,
            Buffer::Indirect{desc_lst, ctrl_desc, last_write, len} => {
                let mut arr = Vec::with_capacity(*len);
                
                for desc in desc_lst.iter() {
                    arr.append(&mut desc.cpy_into_vec());
                }
                arr.into_boxed_slice()
            } ,
        }
    }

    fn scat_cpy (&self) -> Box<[Box<[u8]>]> {
        match &self {
            Buffer::Single{desc_lst, last_write, len} => {
                let mut arr = Vec::with_capacity(desc_lst.len());
                
                for desc in desc_lst.iter() {
                    arr.push(desc.cpy_into_box());
                }
                arr.into_boxed_slice()
            } ,
            Buffer::Multiple{desc_lst, last_write, len} => {
                let mut arr = Vec::with_capacity(desc_lst.len());
                
                for desc in desc_lst.iter() {
                    arr.push(desc.cpy_into_box());
                }
                arr.into_boxed_slice()
            } ,
            Buffer::Indirect{desc_lst, ctrl_desc, last_write, len} => {
                let mut arr = Vec::with_capacity(desc_lst.len());
                
                for desc in desc_lst.iter() {
                    arr.push(desc.cpy_into_box());
                }
                arr.into_boxed_slice()
            } ,
        }
    }

    /// Retruns the number of descriptors inside a buffer.
    fn num_descr(&self ) -> usize {
        match &self {
            Buffer::Single{desc_lst, last_write, len} => desc_lst.len(),
            Buffer::Multiple{desc_lst, last_write, len} => desc_lst.len(),
            Buffer::Indirect{desc_lst, ctrl_desc, last_write, len} => desc_lst.len(),
        }
    }

    /// Returns the overall number of bytes in this Buffer
    fn len(&self) -> usize {
        match &self {
            Buffer::Single{desc_lst, last_write, len} => *len,
            Buffer::Multiple{desc_lst, last_write, len} => *len,
            Buffer::Indirect{desc_lst, ctrl_desc, last_write, len} => *len,
        }
    }

    /// Returns the complete Buffer as a mutable slice of MemDescr, which themselves deref into a `&mut [u8]`.
    ///
    /// As Buffers are able to consist of multiple descriptors
    /// this will return one element 
    /// (`&mut [u8]`) for each descriptor.
    fn as_mut_slice(&mut self) -> &mut [MemDescr] {
        match self {
            Buffer::Single{desc_lst, last_write, len} => desc_lst.as_mut(),
            Buffer::Multiple{desc_lst, last_write, len} => desc_lst.as_mut(),
            Buffer::Indirect{desc_lst, ctrl_desc, last_write, len} => desc_lst.as_mut(),
        } 
    }

    /// Returns the complete Buffer as a slice of MemDescr, which themselves deref into a `&[u8]`.
    ///
    /// As Buffers are able to consist of multiple descriptors
    /// this will return one element 
    /// (`&[u8]`) for each descriptor.
    fn as_slice(&self) -> &[MemDescr] {
        match self {
            Buffer::Single{desc_lst, last_write, len} => desc_lst.as_ref(),
            Buffer::Multiple{desc_lst, last_write, len} => desc_lst.as_ref(),
            Buffer::Indirect{desc_lst, ctrl_desc, last_write, len} => desc_lst.as_ref(),
        }
    }


}

/// Describes a chunk of heap allocated memory. This memory chunk will 
/// be valid until this descriptor is dropped. 
///
/// **Detailed INFOS:**
/// * Sometimes it is necessary to refer to some memory areas which are not
/// controlled by the kernel space or rather by someone else. In these
/// cases the `MemDesc` field `dealloc: bool` allows to prevent the deallocation
/// during drop of the object. 
struct MemDescr {
    /// Points to the controlled memory area
    ptr: *mut u8,
    /// Defines the length of the controlled memory area
    /// starting a `ptr: *mut u8`
    len: usize,
    /// Memory is creaeted via vectors to_raw_parts()
    /// function and transformed back into tracked mempry
    /// via from_raw_parts.
    /// Allthouhg it is unlikely to be important, as the vector will be
    /// "full" (at cap == len) when leaeked, this is for safety reasons
    _cap: usize,
    /// If `id == None` this is an untracked memory descriptor
    /// * Meaining: The descriptor does NOT count as a descriptor 
    /// taken from the [MemPool](MemPool).
    id: Option<MemDescrId>,
    /// Refers to the controlling [memory pool](MemPool)
    pool: Rc<MemPool>,
    /// Controls wether the memory area is deallocated 
    /// upon drop. 
    /// * Should NEVER be set to true, when false.
    ///   * As false will be set after creation and indicates
    ///     that someone else is "controlling" area and takes
    ///     of deallocation.
    /// * Default is true.
    dealloc: bool,
}

impl MemDescr {
    /// Provides a handle to the given memory area by
    /// giving a Box ownership to it.
    fn into_boxed(mut self) -> Box<[u8]> {
        // Prevent double frees, as ownership will be tracked by 
        // Box from now on.
        self.dealloc = false;

        unsafe {
            Vec::from_raw_parts(self.ptr, self.len, self._cap).into_boxed_slice()
        }
    }

    /// Copies the given memory area into a Vector.
    fn cpy_into_vec(&self) -> Vec<u8> {
        let mut vec = vec![0u8;self.len];
        vec.copy_from_slice(&self);
        vec
    }

    /// Copies the given memory area into a Box.
    fn cpy_into_box(&self) -> Box<[u8]> {
        let mut vec = vec![0u8;self.len];
        vec.copy_from_slice(&self);
        vec.into_boxed_slice() 
    }

    /// Returns the raw pointer from where the controlled 
    /// memory area starts.
    fn raw_ptr(&self) -> *mut u8 {
        self.ptr
    }

    /// Returns the length of the controlled memory area.
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
    /// indirect buffer. See [Buffer](Buffer) `Buffer::Indirect` for details!
    fn no_dealloc_clone(&self) -> Self {
        MemDescr {
            ptr: self.ptr,
            len: self.len,
            _cap: self._cap,
            id: None,
            pool: Rc::clone(&self.pool),
            dealloc: false,
        }
    }
}

impl Deref for MemDescr {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        unsafe {
            core::slice::from_raw_parts(self.ptr, self.len)
        }
    }
}

impl DerefMut for MemDescr {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            core::slice::from_raw_parts_mut(self.ptr, self.len)
        }
    }
}


impl Drop for MemDescr {
    fn drop(&mut self) {
        // Handle returning of Id's to pool
        match &self.id {
            Some(id) => {
                let id = self.id.take().unwrap();
                self.pool.ret_id(id);
            },
            None => (),
        }

        if self.dealloc {
            unsafe{
                Vec::from_raw_parts(self.ptr, self.len, self.len);
            }
        }
    }
}

/// A newtype for descriptor ids, for better readability.
struct MemDescrId(u16);

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
        // Usize is as maximum 32bit large. Smaller is not a probelm for the queue
            Some(Bytes(size))
        } else if core::mem::size_of_val(&size) == core::mem::size_of::<u64>(){
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

/// MemPool allows to easily control, request and provide memory for Virtqueues. 
///
/// * The struct is initalized with a limit of free running "tracked" (see `fn pull_untracked`) 
/// memory descriptors. As Virtqueus do only allow a limited amount of descriptors in their queue, 
/// the independent queues, can control the number of descriptors by this.
/// * Furthermore the MemPool struct provides an interface to easily retrieve memory of a wanted size
/// via its `fn pull()`and `fn pull_untracked()` functions. 
/// The functions return a (MemDescr)[MemDescr] which provides an interface to read and write memory safely and handles clean up of memory
/// upon beeing dropped.
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

        for i in 1..(size+1) {
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
    /// * One should set the dealloc parameter of the function to `false` ONLY
    /// when the given memory is controlled somewhere else.
    /// * The given slice MUST be a heap allocated slice.
    ///
    /// **Properties of Returned MemDescr:**
    /// 
    /// * The descriptor will consume one element of the pool. 
    /// * The refered to memory area will be deallocated upon drop
    ///   * Unless the field: `dealloc` is set to `false`.
    ///   * OR the dealloc field parameter is set to false.
    fn pull_from(&self, rc_self: Rc<MemPool>, slice: &[u8], dealloc: bool) -> Result<MemDescr, VirtqError> {
        // Zero sized descriptors are NOT allowed
        assert!(slice.len() != 0);

        let desc_id = match self.pool.borrow_mut().pop() {
            Some(id) => id,
            None => return Err(VirtqError::NoDescrAvail),
        };

        Ok(MemDescr{
            ptr: (&slice[0] as *const u8) as *mut u8,
            len: slice.len(),
            _cap: slice.len(),
            id: Some(desc_id),
            dealloc,
            pool: rc_self,
        })
    }

    /// Creates a MemDescr which refers to already existing memory.
    /// The MemDescr does NOT consume a place in the pool and should
    /// be used with `Buffer::Indirect`.
    ///
    /// **Info on Usage:**
    /// * `Panics` if given `slice.len() == 0`
    /// * One should set the dealloc parameter of the function to `false` ONLY
    /// when the given memory is controlled somewhere else.
    /// * The given slice MUST be a heap allocated slice.
    ///
    /// **Properties of Returned MemDescr:**
    /// 
    /// * The descriptor will consume one element of the pool. 
    /// * The refered to memory area will be deallocated upon drop
    ///   * Unless the field: `dealloc` is set to `false`.
    ///   * OR the dealloc field parameter is set to false.
    fn pull_from_untracked(&self, rc_self: Rc<MemPool>, slice: &[u8], dealloc: bool) -> MemDescr {
        // Zero sized descriptors are NOT allowed
        assert!(slice.len() != 0);

        MemDescr{
            ptr: (&slice[0] as *const u8) as *mut u8,
            len: slice.len(),
            _cap: slice.len(),
            id: None,
            dealloc,
            pool: rc_self,
        }
    }

    /// Pulls a memory descriptor, which owns a memory area of the specified size in bytes. The 
    /// descriptor does consume an ID and hence reduces the amount of descriptors left in the pool by one.
    ///
    /// **INFO:**
    /// * Fails (returns VirtqError), if the pool is empty.
    /// * ID`s of descriptor are by no means sorted. A descriptor can contain an ID between 1 and size_of_pool. 
    /// * Calleys can NOT rely on the next pulled descriptor to contain the subsequent ID after the previously
    ///  pulled descriptor. 
    ///  In essence this means MemDesc can contain arbitrary ID's. E.g.: 
    ///   * First MemPool.pull -> MemDesc with id = 3
    ///   * Second MemPool.pull -> MemDesc with id = 100
    ///   * Third MemPool.pull -> MemDesc with id = 2,
    fn pull(&self, rc_self: Rc<MemPool>, bytes: Bytes) -> Result<MemDescr, VirtqError> {
        let id = match self.pool.borrow_mut().pop() {
            Some(id) => id,
            None => return Err(VirtqError::NoDescrAvail),
        };

        // Allocate heap memory via a vec, leak and cast
        let (ptr, len, cap) = vec![0u8; bytes.0].into_raw_parts();

        Ok(MemDescr {
            ptr,
            len,
            _cap: cap,
            id: Some(id),
            dealloc: true,
            pool: rc_self,
        })
    }
    
    /// Pulls a memory descriptor, which owns a memory area of the specified size in bytes. The 
    /// descriptor consums NO ID and hence DOES NOT reduce the amount of descriptors left in the pool.
    /// * ID`s of descriptor are by no means sorted. A descriptor can contain an ID between 1 and size_of_pool. 
    /// * Calleys can NOT rely on the next pulled descriptor to contain the subsequent ID after the previously
    ///  pulled descriptor. 
    ///  In essence this means MemDesc can contain arbitrary ID's. E.g.: 
    ///   * First MemPool.pull -> MemDesc with id = 3
    ///   * Second MemPool.pull -> MemDesc with id = 100
    ///   * Third MemPool.pull -> MemDesc with id = 2,
    fn pull_untracked(&self, rc_self: Rc<MemPool>, bytes: Bytes)-> MemDescr {
        // Allocate heap memory via a vec, leak and cast
        let (ptr, len, cap) = vec![0u8; bytes.0].into_raw_parts();

        MemDescr {
            ptr,
            len,
            _cap: cap,
            id: None,
            dealloc: true,
            pool: rc_self,
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
pub enum BuffSpec<'a>{
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
    Indirect(&'a [(Bytes)]),
}

/// Ensures `T` is pinned at the same memory location. 
/// This allows to refer to structures via raw pointers.
///
/// **WARN:**
///
/// Assuming a raw pointer `*mut T / *const T` is valid, is only safe as long as 
/// the `Pinned<T>` does life!
/// 
/// **Properties:**
///
/// * `Pinned<T>` behaves like T and implements `Deref`.
/// *  Drops `T` upon drop.
struct Pinned<T>{
    raw_ptr: *mut T,
    // This marker might be needed when drop is implemented.
    //_marker: PhantomData<T>,
}

impl<T: Sized> Pinned<T> {
    /// Creates a new pinned `T` by boxing and leaking it.
    /// Be aware that this will result in a new heap allocation
    /// for `T` to be boxed.
    fn new (val: T)  -> Pinned<T>{
        let boxed = Box::new(val);
        Pinned {
            raw_ptr: Box::into_raw(boxed),
        }
    }

    /// Creates a new pinned `T` from a boxed `T`.
    fn from_boxed(boxed: Box<T>) -> Pinned<T> {
        Pinned {
            raw_ptr: Box::into_raw(boxed)
        }
    }
    
    /// Unpins the pinned value and returns it. This is only
    /// save as long as no one relies on the
    /// memory location of `T`, as this location
    /// will no longer be constant.
    fn into_inner(self) -> T {
        unsafe {
            *Box::from_raw(self.raw_ptr)
        }
    }

    /// Returns a pointer to `T`. The pointer
    /// can be assumed to be constant over the lifetime of 
    /// `Pinned<T>`.
    fn raw_addr(&self) -> *mut T {
        self.raw_ptr
    }
}

impl<T> Deref for Pinned<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe {
            & *(self.raw_ptr)
        }
    }
}

impl<T> Drop for Pinned<T> {
    fn drop(&mut self) {
        unsafe {
            Box::from_raw(self.raw_ptr);
        }
    }
}

//impl <K> Deref for Pinned<Vec<K>>  {
//   type Target = [K];
//   fn deref(&self) -> &Self::Target {
//       let vec = unsafe {
//           & *(self.raw_ptr)
//       };
//
//       vec.as_slice()
//   }
//}


/// Virtqueue descr flags as defined in the specfication.
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
    use super::Transfer;

    pub enum VirtqError {
        General,
        /// Indirect is mixed with Direct descriptors, which is not allowed
        /// according to the specification.
        /// See [Buffer](Buffer) and [BuffSpec](BuffSpec) for details
        BufferInWithDirect,
        /// Call to create a BufferToken or TransferToken without 
        /// any buffers to be inserted
        BufferNotSpecified,
        /// Selected queue does not exist or
        /// is not known to the device and hence can not be used
        QueueNotExisting(u16),
        /// Signals, that the queue does not have any free desciptors
        /// left.
        /// Typically this means, that the driver either has to provide
        /// "unsend" `TransferToken` to the queue (see Docs for details)
        /// or the device needs to process available descriptors in the queue.
        NoDescrAvail,
        /// Indicates that a [BuffSpec](super.BuffSpec) does have the right size
        /// for a given structure. Returns the structures size in bytes.
        /// 
        /// E.g: A struct `T` with size of `4 bytes` must have a `BuffSpec`, which 
        /// defines exactly 4 bytes. Regardeless of wether it is a `Single`, `Multiple`
        /// or `Indirect` BuffSpec.
        BufferSizeWrong(usize),
        /// The requested BufferToken for reuse is signed as not reusable and hence 
        /// can not be used twice.
        /// Typically this is the case if one created the BufferToken indirectly
        /// via `Virtq.prep_transfer_from_raw()`. Due to the fact, that reusing 
        /// Buffers which refer to raw pointers seems dangerours, this is forbidden.
        NoReuseBuffer,
        /// Indicates that a Transfer method was called, that is only allowed to be 
        /// called when the transfer is Finished (or Ready, allthough this state is 
        /// only allowed for Transfer structs owned by the Virtqueue). 
        /// The Error returns the called Transfer for recovery, if called from a 
        /// consuming function as a `Some(Transfer)`. For non-consuming 
        /// functions returns `None`.
        OngoingTransfer(Option<Transfer>),
        /// Indicates a write into a Buffer that is not existing
        NoBufferAvail
    }
}
