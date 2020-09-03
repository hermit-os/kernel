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

use core::ops::{BitAnd, Deref, DerefMut};

/// A usize newtype. If instantiated via ``VqIndex::from(T)``, the newtype is ensured to be
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

/// A usize newtype. If instantiated via ``VqSize::from(T)``, the newtype is ensured to be
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
pub enum Virtq<'a> {
    Packed(PackedVq<'a>),
    Split(SplitVq),
}

// private interface of Virtq
impl<'a> Virtq<'a> {
    /// Dispatches a batch of TransferTokens. The actuall behaviour depends on the respective 
    /// virtqueue implementation.
    fn batched(&mut self, tkns: Vec<TransferToken>) -> Transfer<'a> {
        match self {
            Virtq::Packed(vq) => unimplemented!(),
            Virtq::Split(vq) => unimplemented!(),
        }
    } 
}

// Public interface of Virtq
impl<'a> Virtq<'a> {
    /// Creates a new Virtq of the specified (VqType)[VqType], (VqSize)[VqSize] and the (VqIndex)[VqIndex]. 
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
            Virtq::Packed(vq) => unimplemented!(),
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
    pub fn prep_transfer<T: AsSliceU8 + 'static, K: AsSliceU8 + 'static>(&'a self, send: Option<(Box<T>, BuffSpec)>, recv: Option<(Box<K>, BuffSpec)>) -> Result<TransferToken<'a>, VirtqError> {
            match self {
                Virtq::Packed(vq) => vq.prep_transfer(self ,send, recv),
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
    pub fn prep_transfer_from_raw<T: AsSliceU8, K: AsSliceU8>(&'a self, send: Option<(*mut T, BuffSpec)>, recv: Option<(*mut K, BuffSpec)>) -> Result<TransferToken<'a>, VirtqError> {
        match self {
            Virtq::Packed(vq) => unimplemented!(),
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
    pub fn prep_buffer(&'a self, send: Option<BuffSpec>, recv: Option<BuffSpec>) -> Result<BufferToken<'a>, VirtqError> {
        match self {
            Virtq::Packed(vq) => vq.prep_buffer(&self, send, recv),
            Virtq::Split(vq ) => unimplemented!(),
        }
    }

    /// Early drop provides a mechanism for the queue to detect, if an ongoing transfer or a transfer not yet polled by the driver 
    /// has been dropped. The queue implementation is responsible to taking care what should happen to the respective TransferToken
    /// and BufferToken.
    pub fn early_drop(&self, transfer_tk: Pinned<TransferToken<'a>>) {
        match self {
            Virtq::Packed(vq) => unimplemented!(),
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
    /// * The slice must little endian coded in order to be understood by the device
    /// * The slice must serialize the actual structure the device expects, as the queue will use 
    /// the addresses of the slice in order to refer to the structure.
    unsafe fn as_slice_u8(&self) -> &[u8] {
        core::slice::from_raw_parts(
           (self as *const Self) as *const u8,
            core::mem::size_of_val(&self),
        )
    }

    /// Returns a mutable slice of the given structure.
    ///
    /// ** WARN:**
    /// * The slice must little endian coded in order to be understood by the device
    /// * The slice must serialize the actual structure the device expects, as the queue will use 
    /// the addresses of the slice in order to refer to the structure.
    unsafe fn as_slice_u8_mut(&self) -> &mut [u8] {
        core::slice::from_raw_parts_mut(
            (self as *const Self) as *mut u8,
             core::mem::size_of_val(&self)
        )
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
pub struct Transfer<'a> {
    /// Needs to be Option<Pinned<TransferToken>> in order to prevent deallocation via None
    // See custom drop function for clarity
    transfer_tk: Option<Pinned<TransferToken<'a>>>, 
    vq: &'a Virtq<'a>
}

impl<'a> Drop for Transfer<'a> {
    /// When an unclosed transfer is dropped. The [Pinned](Pinned)<[TransferToken](struct.TransferToken.html)> is returned to the respective
    /// virtqueue, who is responsible of handling these situations. 
    fn drop(&mut self) {
        if let Some(tkn) = self.transfer_tk.take() {
            self.vq.early_drop(tkn)
        }
    }
}

impl<'a> Transfer<'a> {
    /// Used to poll the current state of the transfer.
    /// * true = Transfer is finished and can be closed, reused or return data
    /// * false = Transfer is ongoing
    pub fn poll(&self)-> bool {
        unimplemented!();
    }

    /// Returns a copy if the respective send and receiving buffers
    /// The actul buffers remain in the BufferToken and hence the token can be 
    /// reused afterwards.
    /// 
    /// **Return Tuple**
    ///
    /// `(sended_data, received_data)`
    pub fn ret_cpy(&self) -> (Option<Box<[u8]>>, Option<Box<[u8]>>){
        unimplemented!();
        // Returns a copy of the content
        // Transfer can later be resused via the reuse function
    } 

    /// Returns the actual send and receiving buffers.
    /// The function consumes the tranfer and cleans up all tokens.
    /// 
    /// **Return Tuple**
    ///
    /// `(sended_data, received_data)`
    pub fn ret(mut self) -> (Option<Box<[u8]>>, Option<Box<[u8]>>) {
        unimplemented!();
        // Returns the buffers as RetBuffers, which 
        // NOT allowed if returnable == false
    }

    /// Closes an transfer. If the transfer was ongoing the respective transfer token will be returned to the virtqueue.
    /// If it was finished the resources will be cleaned up.
    pub fn close(mut self) {
        // Consuming function which closes transfer. Simply drops transfer, which handles clean up of data.
    }

    /// If the transfer was finished returns the BufferToken inside the transfer else returns an error.
    pub fn reuse(mut self) -> Result<BufferToken<'a>, VirtqError> {
        // Should I return BufferToken or Buffers which could then be uses via prep buffers?
        unimplemented!();
        // Returns a the buffer token, consumes the transfer.
        // Maybe return transfer again if it is ongoing to allowe recovery if falsely called?
        // - VirtqError would then trigger clean up of transfer when beeing dropped itself
        // NOT allowed if reusable == false
    }

}

/// Enum indicates the current state of a transfer. 
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
pub struct TransferToken<'a> {
    state: TransferState,
    buff_tkn: BufferToken<'a>,
    vq: &'a Virtq<'a>,
}

impl <'a> TransferToken<'a> {
    /// Allows the batched transfer of Transfer Tokens.
    /// The respective buffers will be placed into the queue in sequence and when 
    /// the last one is placed, the queue notifies the queue.
    ///
    /// INFO: This function is part of TransferToken as the tokens can in theory be 
    /// form different queues, so this ensures, the right queue is called.
    pub fn dispatch_batched(self, other: Vec<TransferToken>) -> Vec<Transfer<'a>> {
        unimplemented!();
        // sort virtques after indexes and then call their respective batched function
        // return all as transfer.
    }

    /// Dispatches the provided TransferToken to the respective queue and returns a transfer.
    pub fn dispatch(self) -> Transfer<'a> {
        unimplemented!();
    }

    /// Dispatches the provided TransferToken to the respectuve queue and does 
    /// return when, the queue finished the transfer.
    ///
    /// The resultaing [TransferState](TransferState) in this case is of course 
    /// finished and the returned [Transfer](Transfer) can be reused, copyied from
    /// or retrun the underlying buffers.
    /// Allthough it is recomended to ensure the finished state via`Transfer.poll()` beforehand.
    pub fn dispatch_blocking(&self) -> Result<Transfer<'a>, VirtqError> {
        unimplemented!();
        // Transfer is finished and can be directly polled to return the eventual data.
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
pub struct BufferToken<'a> {
    send_buff: Option<Buffer<'a>>,
    //send_desc_lst: Option<Vec<usize>>,

    recv_buff: Option<Buffer<'a>>,
    //recv_desc_lst: Option<Vec<usize>>,

    vq: &'a Virtq<'a>,
    /// Indicates wether the buff is returnable
    ret_send: bool,
    ret_recv: bool,
    /// Indicates if the token is allowed 
    /// to be reused.
    reusable: bool,
}

// Private Interface of BufferToken
impl <'a> BufferToken<'a> {
    /// A new function to return a Buffertoken. This is needed in order to let rust know 
    /// the correct lifetime of the Virtq reference.
    fn new(send_buff: Option<Buffer<'a>>, recv_buff: Option<Buffer<'a>>, vq: &'a Virtq<'a>, ret_send: bool, ret_recv: bool, reusable: bool) -> BufferToken<'a> {
        BufferToken {
            send_buff,
            recv_buff,
            vq,
            ret_send,
            ret_recv,
            reusable
        }
    }
}

// Public interface of BufferToken
impl<'a> BufferToken<'a> {
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
    pub fn write<K: AsSliceU8, H: AsSliceU8>(self, send: Option<K>, recv: Option<H>) -> Result<TransferToken<'a>, VirtqError> {
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
    pub fn write_seq<K: AsSliceU8, H: AsSliceU8>(send_seq: Option<K>, recv_seq: Option<H>) -> Result<Bytes, VirtqError> {
        unimplemented!();
        // writes K into the first send_buff element. Next write will be written to the next buff.
    }

    /// Consumes the [BufferToken](BufferToken) and returns a [TransferToken](TransferToken), that can be used to actually start the transfer.
    /// 
    /// After this call, the buffers are no longer writable. 
    pub fn provide(mut self) -> TransferToken<'a> {
        unreachable!();
    }
}

/// Describes the type of a buffer and unifies them.
enum Buffer<'a>{
    /// A buffer consisting of a single [Memory Descriptor](MemDescr).
    Single(MemDescr<'a>),
    /// A buffer consisting of a chain of [Memory Descriptors](MemDescr). 
    /// Especially useful if one wants to send multiple structures to a device,
    /// as he can sequentially write (see [BufferToken](BufferToken) `write_seq()`)
    /// those structs into the descriptors.
    Multiple(Vec<MemDescr<'a>>),
    /// A buffer consisting of a single descriptor in the actuall virtqueue,
    /// referencing a list of descriptors somewhere in memory. 
    /// Especially useful of one wants to extend the capacity of the virtqueue.
    /// Also has the same advantages as a `Buffer::Multiple`.
    Indirect((MemDescr<'a>, Vec<MemDescr<'a>>)),
}

// Private Interface of Buffer
impl<'a> Buffer <'a> {
    
    /// Sets a [Pinned](Pinned)<[TransferToken](TransferToken)> for a Buffer. This is 
    /// useful if one wants to create a connection between the complex control structure
    /// of the [Virtq](Virtq) and the control structures defined by the standard. In essence
    /// it allows to store a reference between `TransferToken` and `Descriptors` (Descriptors)
    /// used in the actual raw virtqueue. See Virtio specification v1.1 *Descriptor Area* for 
    /// both queues.
    fn set_ctrl_tkn(&mut self, tkn: &'a Pinned<TransferToken<'a>>) {
        match self {
            Buffer::Single(descr) => descr.set_ctrl_tkn(tkn),
            Buffer::Multiple(descr_lst) => {
                for descr in descr_lst {
                    descr.set_ctrl_tkn(tkn);
                }
            }
            Buffer::Indirect((vq_desc, descr_lst)) => {
                vq_desc.set_ctrl_tkn(tkn);
                for descr in descr_lst {
                    descr.set_ctrl_tkn(tkn);
                }
            } 
        }
    }

    /// Retruns the number of descriptors inside a buffer.
    fn num_descr(&self ) -> usize {
        match self {
            Buffer::Single(_) => 1,
            Buffer::Multiple(desc_lst) => desc_lst.len(),
            Buffer::Indirect((_, desc_lst)) => desc_lst.len(),
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
/// * The Memory area defined by the `ptr: *mut u8`and `len: usize` fields is
/// prepended by a `usize` long memory area. This allows to store a reference
/// to a [Pinned](Pinned)<[TransferToken](TransferToken)> in this area. 
///   * This feature is needed for Virtqueues, in order to operate fast
///   * In cases where it is not used, it does not waste to much memory.
struct MemDescr<'a> {
    /// Points to the controlled memory area
    ptr: *mut u8,
    /// Defines the length of the controlled memory area
    /// starting a `ptr: *mut u8`
    len: usize,
    /// If `id == None` this is an untracked memory descriptor
    /// * Meaining: The descriptor does NOT count as a descriptor 
    /// taken from the [MemPool](MemPool).
    id: Option<MemDescrId>,
    /// Refers to the controlling [memory pool](MemPool)
    pool: &'a MemPool,
    /// Controls wether the memory area is deallocated 
    /// upon drop. 
    /// * Should NEVER be set to true, when false.
    ///   * As false will be set after creation and indicates
    ///     that someone else is "controlling" area and takes
    ///     of deallocation.
    /// * Default is true.
    dealloc: bool,

    /// Reference to the [TransferToken](TransferToken) holding 
    /// the descriptor. 
    /// * If Some: Indicates that the `usize` long prepended 
    /// memory are holds a reference to this `TransferToken`
    ctrl: Option<&'a Pinned<TransferToken<'a>>>
}

impl<'a> MemDescr<'a> {

    /// Sets the contorlling field to hold the given TransferToken
    ///    
    /// MemDescr Pool does allocate one usize before the actual 
    /// Bytes in order to allow a reference to a pinned TransferToken.
    /// This function does allow to set this reference accordingly.
    fn set_ctrl_tkn(&mut self, tkn: &'a Pinned<TransferToken<'a>>) {
        self.ctrl = Some(tkn);
        let ctrl_raw_ptr = self.ctrl.unwrap().raw_addr() as usize;
        let addr_slice = ctrl_raw_ptr.to_ne_bytes();

        // Write address of token into the usized memory area 
        // placed before the memory are where MemDesc.ptr points to.
        // 
        // addr_slice will be of length == ptr_size, always. Hence this
        // direct array indexing is safe.
        let ptr_size = core::mem::size_of::<usize>();
        // Create a negativ counter for offsetting memory area correctly.
        let mut rev_cnt = -(ptr_size as isize);
        for i in 0..ptr_size {
            let tkn_ref = unsafe {
                &mut *(self.ptr.offset(rev_cnt))
            };

            // write into bytes. Behind MemDescr.ptr area.
            *tkn_ref = addr_slice[i];
            // As the writes starts at ptr.offset(- NUM_BYTES_OF_USIZE),
            // the rev_cnt must move "forward" to memory address indicated
            // by self.ptr.
            rev_cnt += 1;
        }
    }

    /// Returns the raw pointer from where the controlled 
    /// memory area starts.
    fn ptr(&self) -> *mut u8 {
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
            id: None,
            pool: self.pool,
            dealloc: false,
            ctrl: self.ctrl,
        }
    }
}

impl<'a> Deref for MemDescr<'a> {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        unsafe {
            core::slice::from_raw_parts(self.ptr, self.len)
        }
    }
}

impl <'a> DerefMut for MemDescr<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            core::slice::from_raw_parts_mut(self.ptr, self.len)
        }
    }
}

impl <'a> Drop for MemDescr<'a> {
    fn drop(&mut self) {
        unimplemented!();
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
    pool: Vec<MemDescrId>,
    limit: u16,
}

impl <'a> MemPool {
    /// Returns a new instance, with a pool of the specified size.
    fn new(size: u16) -> MemPool {
        unimplemented!();
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
    fn pull_from(&self, slice: &[u8], dealloc: bool) -> Result<MemDescr, VirtqError> {
        // Zero sized descriptors are NOT allowed
        assert!(slice.len() != 0);

        let desc_id = match self.pool.pop() {
            Some(id) => id,
            None => return Err(VirtqError::NoDescrAvail),
        };

        Ok(MemDescr{
            ptr: (&slice[0] as *const u8) as *mut u8,
            len: slice.len(),
            id: Some(desc_id),
            dealloc,
            pool: &self,
            ctrl: None,
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
    fn pull_from_untracked(&self, slice: &[u8], dealloc: bool) -> MemDescr {
        // Zero sized descriptors are NOT allowed
        assert!(slice.len() != 0);

        MemDescr{
            ptr: (&slice[0] as *const u8) as *mut u8,
            len: slice.len(),
            id: None,
            dealloc,
            pool: &self,
            ctrl: None,
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
    fn pull (&'a self, bytes: Bytes) -> Result<MemDescr<'a>, VirtqError> {
        let id = match self.pool.pop() {
            Some(id) => id,
            None => return Err(VirtqError::NoDescrAvail),
        };

        // Allocate heap memory via a vec, leak and cast
        let ptr = Box::into_raw(vec![0u8; bytes.0].into_boxed_slice()) as *mut u8;

        Ok(MemDescr {
            ptr,
            len: bytes.0,
            id: Some(id),
            dealloc: true,
            pool: &self,
            ctrl: None,
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
    fn pull_untracked(&'a self, bytes: Bytes)-> MemDescr<'a> {
        // Allocate heap memory via a vec, leak and cast
        let ptr = Box::into_raw(vec![0u8; bytes.0].into_boxed_slice()) as *mut u8;

        MemDescr {
            ptr,
            len: bytes.0,
            id: None,
            dealloc: true,
            pool: &self,
            ctrl: None,
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
pub enum BuffSpec<'a>{
    /// Create a buffer with a single descriptor of size `usize`
    Single(Bytes),
    /// Create a buffer consisting of multiple descriptors, which size is
    /// defined by `usize.`
    Multiple(&'a [Bytes]),
    /// Creates a buffer consisting of multiple descriptors, which size is
    /// defined by `usize`. But consumes only ONE descriptor of the actual 
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
        BufferSizeWrong(usize)
    }
}
