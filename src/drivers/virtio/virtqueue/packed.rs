// Copyright (c) 2020 Frederik Schulz, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! This module contains Virtio's packed virtqueue. 
//! See Virito specification v1.1. - 2.7
use alloc::vec::Vec;

use super::super::transport::pci::ComCfg;
use super::{VqSize, VqIndex, MemPool, MemDescrId, MemDescr, BufferToken, TransferToken, TransferState, Buffer, BuffSpec, Bytes, AsSliceU8, Pinned, Virtq, DescrFlags};
use super::error::VirtqError;
use self::error::VqPackedError;
use core::convert::TryFrom;
use alloc::boxed::Box;

/// A newtype of bool used for convenience in context with 
/// packed queues wrap counter.
///
/// For more details see Virtio specification v1.1. - 2.7.1
#[derive(Copy, Clone, Debug)]
pub struct WrapCount(bool);

impl WrapCount {
    /// Returns a new WrapCount struct initalized to true or 1.
    /// 
    /// See virtio specification v1.1. - 2.7.1
    pub fn new() -> Self {
        WrapCount(true)
    }

    /// Toogles a given wrap count to respectiver other value.
    ///
    /// If WrapCount(true) returns WrapCount(false), 
    /// if WrapCount(false) returns WrapCount(true).
    pub fn wrap(&mut self) {
        if self.0 == false {
            self.0 = true;
        } else {
            self.0 = false;
        }
    }
}

/// Structure which allows to control raw ring and operate easily on it
/// 
/// WARN: NEVER PUSH TO THE RING AFTER DESCRIPTORRING HAS BEEN INITALIZED AS THIS WILL PROBABLY RESULT IN A 
/// RELOCATION OF THE VECTOR AND HENCE THE DEVICE WILL NO LONGER NO THE RINGS ADDRESS!
struct DescriptorRing {
    ring: Pinned<Vec<Descriptor>>, 

    // Controlling variables for the ring
    //
    /// where to insert availble descriptors next
    write_index: u16,
    /// How much descriptors can be inserted
    capacity: u16,
    /// Where to expect the next used descriptor by the device
    poll_index: u16,
    /// See Virtio specification v1.1. - 2.7.1
    wrap_count: WrapCount,
}

impl DescriptorRing {
    fn new(size: u16) -> Self {
        // WARN: Uncatched as usize call here. Could panic if used with usize < u16
        let mut ring = Box::new(Vec::with_capacity(usize::try_from(size).unwrap()));
        for _ in 0..size {
            ring.push(Descriptor {
                address: 0,
                len: 0,
                buff_id: 0,
                flags: 0,
            });
        }
        

        DescriptorRing { 
            ring: Pinned::from_boxed(ring),
            write_index: 0,
            capacity: size,
            poll_index: 0,
            wrap_count: WrapCount::new(),
         }
    }

    /// # Unsafe
    /// Polls last index postiion. If used. use the address and the prepended reference to the 
    /// to return an TransferToken reference. Also sets the poll index to show the next item in list. 
    fn poll(&mut self) -> Option<&TransferToken> {
        unimplemented!();
    }

    fn push() {
        // places a new descriptor into the ring
        // Was soll übergeben werden? Ein Raw element, oder soll der Ring sich auch um das Managen der BufferIds etc. kümmern
        // also ehere einen MemDescr nehmen?
        unimplemented!();
    }

    /// # Unsafe
    /// Returns the memory address of the first element of the descriptor ring
    fn raw_addr(&self) -> usize {
        let temp_ring = unsafe {
            Box::from_raw(self.ring.raw_addr())
        };
        let (ptr, len, cap) = temp_ring.into_raw_parts();
        // return only pointer as usize
        ptr as usize
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
        let mut desc_bytes: [u8; 16] = [0;16];

        // Call to little endian, as device will read this and
        // Virtio devices are inherently little endian coded.
        let mem_addr: [u8;8] = self.address.to_le_bytes();
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
        let id: [u8;2] = self.buff_id.to_le_bytes();
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

    fn read() {
        unimplemented!();
    }

    fn mark_avail(&self) {
        unimplemented!();
    }

    fn is_used() {
        unimplemented!();
    }

    fn is_avail() {
        unimplemented!();
    }

}

/// Driver and device event suppression struct used in packed virtqueues.
///
/// Structure layout see Virtio specification v1.1. - 2.7.14
/// Alignment see Virtio specification v1.1. - 2.7.10.1
#[repr(C, align(4))]
struct EventSuppr {
   event: u16,
   flags: u16, 
}

impl EventSuppr {
    /// Returns a zero initalized EventSuppr structure
    fn new() -> Self {
        EventSuppr {
            event: 0,
            flags: 0,
        }
    }
    
    /// Enables notifications by setting the LSB.
    /// See Virito specification v1.1. - 2.7.10
    fn enable_notif() {
        unimplemented!();
    }

    /// Disables notifications by unsetting the LSB.
    /// See Virtio specification v1.1. - 2.7.10
    fn disable_notif() {
        unimplemented!();
    }

    /// Reads notification bit (i.e. LSB) and returns value.
    /// If notifications are enabled returns true, else false.
    fn is_notif() -> bool {
        unimplemented!();
    }


    fn enable_specific(descriptor_id: u16, on_count: WrapCount) {
        // Check if VIRTIO_F_RING_EVENT_IDX has been negotiated

        // Check if descriptor_id is below 2^15

        // Set second bit from LSB to true

        // Set descriptor id, triggering notification

        // Set which wrap counter triggers

        unimplemented!();
    }
}

/// Packed virtqueue which provides the functionilaty as described in the 
/// virtio specification v1.1. - 2.7
pub struct PackedVq<'a> {
    /// Ring which allows easy access to the raw ring structure of the 
    /// specfification
    descr_ring: DescriptorRing,
    /// Raw EventSuppr structure
    drv_event: Pinned<EventSuppr>,
    /// Raw
    dev_event: Pinned<EventSuppr>,
    /// Memory pool controls the amount of "free floating" descriptors
    /// See [MemPool](super.MemPool) docs for detail.
    mem_pool: MemPool,
    /// The size of the queue, equals the number of descriptors which can
    /// be used
    size: u16,
    /// Holds all erly dropped `TransferToken`
    /// If `TransferToken.state == TransferState::Finished`
    /// the Token can be safely dropped
    dropped: Vec<TransferToken<'a>>,
}

// Public interface of PackedVq
impl<'a> PackedVq<'a> {
    pub fn new(com_cfg: &mut ComCfg, size: VqSize, index: VqIndex) -> Result<Self, VqPackedError> {
        // Get a handler to the queues configuration area.
        let mut vq_handler = match com_cfg.select_vq(index.into()) {
            Some(handler) => handler,
            None => return Err(VqPackedError::QueueNotExisting(index.into())),
        };

        // Must catch zero size as it is not allowed for packed queues.
        // Must catch size larger 32768 (2^15) as it is not allowed for packed queues.
        //
        // See Virtio specification v1.1. - 4.1.4.3.2
        let vq_size;
        if (size.0 == 0) | (size.0 > 32768) {
            return Err(VqPackedError::SizeNotAllowed(size.0));
        } else {
            vq_size = vq_handler.set_vq_size(size.0);
        }
        
        let descr_ring = DescriptorRing::new(vq_size);
        let drv_event = Pinned::new(EventSuppr::new());
        let dev_event= Pinned::new(EventSuppr::new());

        // Provide memory areas of the queues data structures to the device
        vq_handler.set_ring_addr(index.into(), descr_ring.raw_addr());
        // As usize is safe here, as the *mut EventSuppr raw pointer is a thin pointer of size usize
        vq_handler.set_drv_ctrl_addr(index.into(), drv_event.raw_addr() as usize);
        vq_handler.set_dev_ctrl_addr(index.into(), dev_event.raw_addr() as usize);


        // Initalize new memory pool.
        let mem_pool = MemPool::new(size.0);

        // Initalize an empty vector for future dropped transfers
        let dropped: Vec<TransferToken> = Vec::new();

        Ok(PackedVq {
            descr_ring,
            drv_event, 
            dev_event, 
            mem_pool,
            size: size.into(),
            dropped,
        })
    }

    /// See `Virtq.prep_transfer()` documentation.
    pub fn prep_transfer<'b, T: AsSliceU8 + 'static, K: AsSliceU8 + 'static>(&self, master: &'b Virtq<'b>, send: Option<(Box<T>, BuffSpec)>, recv: Option<(Box<K>, BuffSpec)>) 
        -> Result<TransferToken<'b>, VirtqError> {
        match (send, recv) {
            (None, None) => return Err(VirtqError::BufferNotSpecified),
            (Some((send_data, send_spec)), None) => {
                match send_spec {
                    BuffSpec::Single(size) => {
                        let data_slice = unsafe {send_data.as_slice_u8()};

                        // Buffer must have the right size
                        if data_slice.len() != size.into() {
                            return Err(VirtqError::BufferSizeWrong(data_slice.len()))
                        }

                        let desc = match self.mem_pool.pull_from(data_slice,true) {
                            Ok(desc) => desc,
                            Err(vq_err) => return Err(vq_err),
                        };

                        // Leak the box, as the memory will be deallocated upon drop of MemDescr
                        Box::leak(send_data);

                        let buff_tkn = BufferToken::new(
                            Some(Buffer::Single(desc)),
                            None,
                            master,
                            true,
                            true,
                            true,
                        );

                        Ok(TransferToken{
                            state: TransferState::Ready,
                            buff_tkn,
                            vq: master,
                        })
                    },
                    BuffSpec::Multiple(size_lst) => {
                        let data_slice = unsafe {send_data.as_slice_u8()};
                        let mut desc_lst: Vec<MemDescr> = Vec::with_capacity(size_lst.len());
                        let mut index = 0usize;

                        for byte in size_lst {
                            let end_index = index + usize::from(*byte);
                            let next_slice = match data_slice.get(index..end_index){
                                Some(slice) => slice, 
                                None => return Err(VirtqError::BufferSizeWrong(data_slice.len())),
                            };

                            match self.mem_pool.pull_from(next_slice, true) {
                                Ok(desc) => desc_lst.push(desc),
                                Err(vq_err) => return Err(vq_err),
                            };

                            // update the starting index for the next iteration
                            index = index + usize::from(*byte);
                        }

                       // Leak the box, as the memory will be deallocated upon drop of MemDescr
                       Box::leak(send_data); 

                        Ok(TransferToken{
                            state: TransferState::Ready,
                            buff_tkn: BufferToken {
                                send_buff: Some(Buffer::Multiple(desc_lst)),
                                recv_buff: None,
                                vq: master,
                                ret_send: true,
                                ret_recv: true,
                                reusable: true,
                            },
                            vq: master,
                        })
                    },
                    BuffSpec::Indirect(size_lst) => {
                        let data_slice = unsafe {send_data.as_slice_u8()};
                        let mut desc_lst: Vec<MemDescr> = Vec::with_capacity(size_lst.len());
                        let mut index = 0usize;

                        for byte in size_lst {
                            let end_index = index + usize::from(*byte);
                            let next_slice = match data_slice.get(index..end_index){
                                Some(slice) => slice, 
                                None => return Err(VirtqError::BufferSizeWrong(data_slice.len())),
                            };

                            desc_lst.push(self.mem_pool.pull_from_untracked(next_slice, true));

                            // update the starting index for the next iteration
                            index = index + usize::from(*byte);
                        }

                        let ctrl_desc = match self.create_indirect_ctrl(master, Some(&desc_lst), None) {
                            Ok(desc) => desc,
                            Err(vq_err) => return Err(vq_err),
                        };

                        // Leak the box, as the memory will be deallocated upon drop of MemDescr
                        Box::leak(send_data);
                        
                        Ok(TransferToken{
                            state: TransferState::Ready,
                            buff_tkn: BufferToken {
                                send_buff: Some(Buffer::Indirect((ctrl_desc,desc_lst))),
                                recv_buff: None,
                                vq: master,
                                ret_send: true,
                                ret_recv: true,
                                reusable: true,
                            },
                            vq: master,
                        })
                    },
                }
            },
            (None, Some((recv_data, recv_spec))) => {
                match recv_spec {
                    BuffSpec::Single(size) => {
                        let data_slice = unsafe {recv_data.as_slice_u8()};

                        // Buffer must have the right size
                        if data_slice.len() != size.into() {
                            return Err(VirtqError::BufferSizeWrong(data_slice.len()))
                        }

                        let desc = match self.mem_pool.pull_from(data_slice,true) {
                            Ok(desc) => desc,
                            Err(vq_err) => return Err(vq_err),
                        };

                        // Leak the box, as the memory will be deallocated upon drop of MemDescr
                        Box::leak(recv_data);

                        Ok(TransferToken{
                            state: TransferState::Ready,
                            buff_tkn: BufferToken {
                                send_buff: None,
                                recv_buff: Some(Buffer::Single(desc)),
                                vq: master,
                                ret_send: true,
                                ret_recv: true,
                                reusable: true,
                            },
                            vq: master,
                        })
                    },
                    BuffSpec::Multiple(size_lst) => {
                        let data_slice = unsafe {recv_data.as_slice_u8()};
                        let mut desc_lst: Vec<MemDescr> = Vec::with_capacity(size_lst.len());
                        let mut index = 0usize;

                        for byte in size_lst {
                            let end_index = index + usize::from(*byte);
                            let next_slice = match data_slice.get(index..end_index){
                                Some(slice) => slice, 
                                None => return Err(VirtqError::BufferSizeWrong(data_slice.len())),
                            };

                            match self.mem_pool.pull_from(next_slice, true) {
                                Ok(desc) => desc_lst.push(desc),
                                Err(vq_err) => return Err(vq_err),
                            };

                            // update the starting index for the next iteration
                            index = index + usize::from(*byte);
                        }

                       // Leak the box, as the memory will be deallocated upon drop of MemDescr
                       Box::leak(recv_data); 

                        Ok(TransferToken{
                            state: TransferState::Ready,
                            buff_tkn: BufferToken {
                                send_buff: None,
                                recv_buff: Some(Buffer::Multiple(desc_lst)),
                                vq: master,
                                ret_send: true,
                                ret_recv: true,
                                reusable: true,
                            },
                            vq: master,
                        })
                    },
                    BuffSpec::Indirect(size_lst) => {
                        let data_slice = unsafe {recv_data.as_slice_u8()};
                        let mut desc_lst: Vec<MemDescr> = Vec::with_capacity(size_lst.len());
                        let mut index = 0usize;

                        for byte in size_lst {
                            let end_index = index + usize::from(*byte);
                            let next_slice = match data_slice.get(index..end_index){
                                Some(slice) => slice, 
                                None => return Err(VirtqError::BufferSizeWrong(data_slice.len())),
                            };

                            desc_lst.push(self.mem_pool.pull_from_untracked(next_slice, true));

                            // update the starting index for the next iteration
                            index = index + usize::from(*byte);
                        }

                        let ctrl_desc = match self.create_indirect_ctrl(master, None, Some(&desc_lst)) {
                            Ok(desc) => desc,
                            Err(vq_err) => return Err(vq_err),
                        };

                        // Leak the box, as the memory will be deallocated upon drop of MemDescr
                        Box::leak(recv_data);
                        
                        Ok(TransferToken{
                            state: TransferState::Ready,
                            buff_tkn: BufferToken {
                                send_buff: None,
                                recv_buff: Some(Buffer::Indirect((ctrl_desc,desc_lst))),
                                vq: master,
                                ret_send: true,
                                ret_recv: true,
                                reusable: true,
                            },
                            vq: master,
                        })
                    },
                }
            },
            (Some((send_data, send_spec)), Some((recv_data, recv_spec))) => {
                match (send_spec, recv_spec) {
                    (BuffSpec::Single(send_size), BuffSpec::Single(recv_size)) => {
                        let send_data_slice = unsafe {send_data.as_slice_u8()};

                        // Buffer must have the right size
                        if send_data_slice.len() != send_size.into() {
                            return Err(VirtqError::BufferSizeWrong(send_data_slice.len()))
                        }

                        let send_desc = match self.mem_pool.pull_from(send_data_slice, true) {
                            Ok(desc) => desc,
                            Err(vq_err) => return Err(vq_err),
                        };

                        // Leak the box, as the memory will be deallocated upon drop of MemDescr
                        Box::leak(send_data);

                        let recv_data_slice = unsafe {recv_data.as_slice_u8()};

                        // Buffer must have the right size
                        if recv_data_slice.len() != recv_size.into() {
                            return Err(VirtqError::BufferSizeWrong(recv_data_slice.len()))
                        }

                        let recv_desc = match self.mem_pool.pull_from(recv_data_slice, true) {
                            Ok(desc) => desc,
                            Err(vq_err) => return Err(vq_err),
                        };

                        // Leak the box, as the memory will be deallocated upon drop of MemDescr
                        Box::leak(recv_data);

                        Ok(TransferToken{
                            state: TransferState::Ready,
                            buff_tkn: BufferToken {
                                send_buff: Some(Buffer::Single(send_desc)),
                                recv_buff: Some(Buffer::Single(recv_desc)),
                                vq: master,
                                ret_send: true,
                                ret_recv: true,
                                reusable: true,
                            },
                            vq: master,
                        })
                    },
                    (BuffSpec::Single(send_size), BuffSpec::Multiple(recv_size_lst)) => {
                        let send_data_slice = unsafe {send_data.as_slice_u8()};

                        // Buffer must have the right size
                        if send_data_slice.len() != send_size.into() {
                            return Err(VirtqError::BufferSizeWrong(send_data_slice.len()))
                        }

                        let send_desc = match self.mem_pool.pull_from(send_data_slice, true) {
                            Ok(desc) => desc,
                            Err(vq_err) => return Err(vq_err),
                        };

                        // Leak the box, as the memory will be deallocated upon drop of MemDescr
                        Box::leak(send_data);

                        let recv_data_slice = unsafe {recv_data.as_slice_u8()};
                        let mut recv_desc_lst: Vec<MemDescr> = Vec::with_capacity(recv_size_lst.len());
                        let mut index = 0usize;

                        for byte in recv_size_lst {
                            let end_index = index + usize::from(*byte);
                            let next_slice = match recv_data_slice.get(index..end_index){
                                Some(slice) => slice, 
                                None => return Err(VirtqError::BufferSizeWrong(recv_data_slice.len())),
                            };

                            match self.mem_pool.pull_from(next_slice, true) {
                                Ok(desc) => recv_desc_lst.push(desc),
                                Err(vq_err) => return Err(vq_err),
                            };

                            // update the starting index for the next iteration
                            index = index + usize::from(*byte);
                        }

                       // Leak the box, as the memory will be deallocated upon drop of MemDescr
                       Box::leak(recv_data);  

                        Ok(TransferToken{
                            state: TransferState::Ready,
                            buff_tkn: BufferToken {
                                send_buff: Some(Buffer::Single(send_desc)),
                                recv_buff: Some(Buffer::Multiple(recv_desc_lst)),
                                vq: master,
                                ret_send: true,
                                ret_recv: true,
                                reusable: true,
                            },
                            vq: master,
                        })
                    },
                    (BuffSpec::Multiple(send_size_lst), BuffSpec::Multiple(recv_size_lst)) => {
                        let send_data_slice = unsafe {send_data.as_slice_u8()};
                        let mut send_desc_lst: Vec<MemDescr> = Vec::with_capacity(send_size_lst.len());
                        let mut index = 0usize;

                        for byte in send_size_lst {
                            let end_index = index + usize::from(*byte);
                            let next_slice = match send_data_slice.get(index..end_index){
                                Some(slice) => slice, 
                                None => return Err(VirtqError::BufferSizeWrong(send_data_slice.len())),
                            };

                            match self.mem_pool.pull_from(next_slice, true) {
                                Ok(desc) => send_desc_lst.push(desc),
                                Err(vq_err) => return Err(vq_err),
                            };

                            // update the starting index for the next iteration
                            index = index + usize::from(*byte);
                        }

                        // Leak the box, as the memory will be deallocated upon drop of MemDescr
                        Box::leak(send_data);  

                        let recv_data_slice = unsafe {recv_data.as_slice_u8()};
                        let mut recv_desc_lst: Vec<MemDescr> = Vec::with_capacity(recv_size_lst.len());
                        let mut index = 0usize;

                        for byte in recv_size_lst {
                            let end_index = index + usize::from(*byte);
                            let next_slice = match recv_data_slice.get(index..end_index){
                                Some(slice) => slice, 
                                None => return Err(VirtqError::BufferSizeWrong(recv_data_slice.len())),
                            };

                            match self.mem_pool.pull_from(next_slice, true) {
                                Ok(desc) => recv_desc_lst.push(desc),
                                Err(vq_err) => return Err(vq_err),
                            };

                            // update the starting index for the next iteration
                            index = index + usize::from(*byte);
                        }

                        // Leak the box, as the memory will be deallocated upon drop of MemDescr
                        Box::leak(recv_data);  

                        Ok(TransferToken{
                            state: TransferState::Ready,
                            buff_tkn: BufferToken {
                                send_buff: Some(Buffer::Multiple(send_desc_lst)),
                                recv_buff: Some(Buffer::Multiple(recv_desc_lst)),
                                vq: master,
                                ret_send: true,
                                ret_recv: true,
                                reusable: true,
                            },
                            vq: master,
                        })
                    },
                    (BuffSpec::Multiple(send_size_lst), BuffSpec::Single(recv_size)) => {
                        let send_data_slice = unsafe {send_data.as_slice_u8()};
                        let mut send_desc_lst: Vec<MemDescr> = Vec::with_capacity(send_size_lst.len());
                        let mut index = 0usize;

                        for byte in send_size_lst {
                            let end_index = index + usize::from(*byte);
                            let next_slice = match send_data_slice.get(index..end_index){
                                Some(slice) => slice, 
                                None => return Err(VirtqError::BufferSizeWrong(send_data_slice.len())),
                            };

                            match self.mem_pool.pull_from(next_slice, true) {
                                Ok(desc) => send_desc_lst.push(desc),
                                Err(vq_err) => return Err(vq_err),
                            };

                            // update the starting index for the next iteration
                            index = index + usize::from(*byte);
                        }

                        // Leak the box, as the memory will be deallocated upon drop of MemDescr
                        Box::leak(send_data);  

                        let recv_data_slice = unsafe {recv_data.as_slice_u8()};

                        // Buffer must have the right size
                        if recv_data_slice.len() != recv_size.into() {
                            return Err(VirtqError::BufferSizeWrong(recv_data_slice.len()))
                        }

                        let recv_desc = match self.mem_pool.pull_from(recv_data_slice, true) {
                            Ok(desc) => desc,
                            Err(vq_err) => return Err(vq_err),
                        };

                        // Leak the box, as the memory will be deallocated upon drop of MemDescr
                        Box::leak(recv_data);

                        Ok(TransferToken{
                            state: TransferState::Ready,
                            buff_tkn: BufferToken {
                                send_buff: Some(Buffer::Multiple(send_desc_lst)),
                                recv_buff: Some(Buffer::Single(recv_desc)),
                                vq: master,
                                ret_send: true,
                                ret_recv: true,
                                reusable: true,
                            },
                            vq: master,
                        })
                    },
                    (BuffSpec::Indirect(send_size_lst), BuffSpec::Indirect(recv_size_lst)) => {
                        let send_data_slice = unsafe {send_data.as_slice_u8()};
                        let mut send_desc_lst: Vec<MemDescr> = Vec::with_capacity(send_size_lst.len());
                        let mut index = 0usize;

                        for byte in send_size_lst {
                            let end_index = index + usize::from(*byte);
                            let next_slice = match send_data_slice.get(index..end_index){
                                Some(slice) => slice, 
                                None => return Err(VirtqError::BufferSizeWrong(send_data_slice.len())),
                            };

                            send_desc_lst.push(self.mem_pool.pull_from_untracked(next_slice, true));

                            // update the starting index for the next iteration
                            index = index + usize::from(*byte);
                        }

                        // Leak the box, as the memory will be deallocated upon drop of MemDescr
                        Box::leak(send_data);  

                        let recv_data_slice = unsafe {recv_data.as_slice_u8()};
                        let mut recv_desc_lst: Vec<MemDescr> = Vec::with_capacity(recv_size_lst.len());
                        let mut index = 0usize;

                        for byte in recv_size_lst {
                            let end_index = index + usize::from(*byte);
                            let next_slice = match recv_data_slice.get(index..end_index){
                                Some(slice) => slice, 
                                None => return Err(VirtqError::BufferSizeWrong(recv_data_slice.len())),
                            };

                            recv_desc_lst.push(self.mem_pool.pull_from_untracked(next_slice, true));

                            // update the starting index for the next iteration
                            index = index + usize::from(*byte);
                        }

                        // Leak the box, as the memory will be deallocated upon drop of MemDescr
                        Box::leak(recv_data);  

                        let ctrl_desc = match self.create_indirect_ctrl(master, Some(&send_desc_lst), Some(&recv_desc_lst)) {
                            Ok(desc) => desc,
                            Err(vq_err) => return Err(vq_err),
                        }; 

                        Ok(TransferToken{
                            state: TransferState::Ready,
                            buff_tkn: BufferToken {
                                recv_buff: Some(Buffer::Indirect((ctrl_desc.no_dealloc_clone(), recv_desc_lst))),
                                send_buff: Some(Buffer::Indirect((ctrl_desc, send_desc_lst))),
                                vq: master,
                                ret_send: true,
                                ret_recv: true,
                                reusable: true,
                            },
                            vq: master,
                        })
                    },
                    (BuffSpec::Indirect(_), BuffSpec::Single(_)) | (BuffSpec::Indirect(_), BuffSpec::Multiple(_)) => {
                        return Err(VirtqError::BufferInWithDirect)
                    },
                    (BuffSpec::Single(_), BuffSpec::Indirect(_)) | (BuffSpec::Multiple(_), BuffSpec::Indirect(_)) => {
                        return Err(VirtqError::BufferInWithDirect)
                    }
                }
            }
        }        
    }

    /// See `Virtq.prep_transfer_from_raw()` documentation.
    pub fn prep_transfer_from_raw<'b, T: AsSliceU8 + 'static, K: AsSliceU8 + 'static>(&self, master: &'b Virtq<'b>, send: Option<(*mut T, BuffSpec)>, recv: Option<(*mut K, BuffSpec)>) 
        -> Result<TransferToken<'b>, VirtqError> {
        match (send, recv) {
            (None, None) => return Err(VirtqError::BufferNotSpecified),
            (Some((send_data, send_spec)), None) => {
                match send_spec {
                    BuffSpec::Single(size) => {
                        let data_slice = unsafe {(*send_data).as_slice_u8()};

                        // Buffer must have the right size
                        if data_slice.len() != size.into() {
                            return Err(VirtqError::BufferSizeWrong(data_slice.len()))
                        }

                        let desc = match self.mem_pool.pull_from(data_slice, false) {
                            Ok(desc) => desc,
                            Err(vq_err) => return Err(vq_err),
                        };

                        Ok(TransferToken{
                            state: TransferState::Ready,
                            buff_tkn: BufferToken {
                                send_buff: Some(Buffer::Single(desc)),
                                recv_buff: None,
                                vq: master,
                                ret_send: false,
                                ret_recv: false,
                                reusable: false,
                            },
                            vq: master,
                        })
                    },
                    BuffSpec::Multiple(size_lst) => {
                        let data_slice = unsafe {(*send_data).as_slice_u8()};
                        let mut desc_lst: Vec<MemDescr> = Vec::with_capacity(size_lst.len());
                        let mut index = 0usize;

                        for byte in size_lst {
                            let end_index = index + usize::from(*byte);
                            let next_slice = match data_slice.get(index..end_index){
                                Some(slice) => slice, 
                                None => return Err(VirtqError::BufferSizeWrong(data_slice.len())),
                            };

                            match self.mem_pool.pull_from(next_slice, false) {
                                Ok(desc) => desc_lst.push(desc),
                                Err(vq_err) => return Err(vq_err),
                            };

                            // update the starting index for the next iteration
                            index = index + usize::from(*byte);
                        } 

                        Ok(TransferToken{
                            state: TransferState::Ready,
                            buff_tkn: BufferToken {
                                send_buff: Some(Buffer::Multiple(desc_lst)),
                                recv_buff: None,
                                vq: master,
                                ret_send: false,
                                ret_recv: false,
                                reusable: false,
                            },
                            vq: master,
                        })
                    },
                    BuffSpec::Indirect(size_lst) => {
                        let data_slice = unsafe {(*send_data).as_slice_u8()};
                        let mut desc_lst: Vec<MemDescr> = Vec::with_capacity(size_lst.len());
                        let mut index = 0usize;

                        for byte in size_lst {
                            let end_index = index + usize::from(*byte);
                            let next_slice = match data_slice.get(index..end_index){
                                Some(slice) => slice, 
                                None => return Err(VirtqError::BufferSizeWrong(data_slice.len())),
                            };

                            desc_lst.push(self.mem_pool.pull_from_untracked(next_slice, false));

                            // update the starting index for the next iteration
                            index = index + usize::from(*byte);
                        }

                        let ctrl_desc = match self.create_indirect_ctrl(master, Some(&desc_lst), None) {
                            Ok(desc) => desc,
                            Err(vq_err) => return Err(vq_err),
                        };

                        Ok(TransferToken{
                            state: TransferState::Ready,
                            buff_tkn: BufferToken {
                                send_buff: Some(Buffer::Indirect((ctrl_desc,desc_lst))),
                                recv_buff: None,
                                vq: master,
                                ret_send: false,
                                ret_recv: false,
                                reusable: false,
                            },
                            vq: master,
                        })
                    },
                }
            },
            (None, Some((recv_data, recv_spec))) => {
                match recv_spec {
                    BuffSpec::Single(size) => {
                        let data_slice = unsafe {(*recv_data).as_slice_u8()};

                        // Buffer must have the right size
                        if data_slice.len() != size.into() {
                            return Err(VirtqError::BufferSizeWrong(data_slice.len()))
                        }

                        let desc = match self.mem_pool.pull_from(data_slice, false) {
                            Ok(desc) => desc,
                            Err(vq_err) => return Err(vq_err),
                        };

                        Ok(TransferToken{
                            state: TransferState::Ready,
                            buff_tkn: BufferToken {
                                send_buff: None,
                                recv_buff: Some(Buffer::Single(desc)),
                                vq: master,
                                ret_send: false,
                                ret_recv: false,
                                reusable: false,
                            },
                            vq: master,
                        })
                    },
                    BuffSpec::Multiple(size_lst) => {
                        let data_slice = unsafe {(*recv_data).as_slice_u8()};
                        let mut desc_lst: Vec<MemDescr> = Vec::with_capacity(size_lst.len());
                        let mut index = 0usize;

                        for byte in size_lst {
                            let end_index = index + usize::from(*byte);
                            let next_slice = match data_slice.get(index..end_index){
                                Some(slice) => slice, 
                                None => return Err(VirtqError::BufferSizeWrong(data_slice.len())),
                            };

                            match self.mem_pool.pull_from(next_slice, false) {
                                Ok(desc) => desc_lst.push(desc),
                                Err(vq_err) => return Err(vq_err),
                            };

                            // update the starting index for the next iteration
                            index = index + usize::from(*byte);
                        }

                        Ok(TransferToken{
                            state: TransferState::Ready,
                            buff_tkn: BufferToken {
                                send_buff: None,
                                recv_buff: Some(Buffer::Multiple(desc_lst)),
                                vq: master,
                                ret_send: false,
                                ret_recv: false,
                                reusable: false,
                            },
                            vq: master,
                        })
                    },
                    BuffSpec::Indirect(size_lst) => {
                        let data_slice = unsafe {(*recv_data).as_slice_u8()};
                        let mut desc_lst: Vec<MemDescr> = Vec::with_capacity(size_lst.len());
                        let mut index = 0usize;

                        for byte in size_lst {
                            let end_index = index + usize::from(*byte);
                            let next_slice = match data_slice.get(index..end_index){
                                Some(slice) => slice, 
                                None => return Err(VirtqError::BufferSizeWrong(data_slice.len())),
                            };

                            desc_lst.push(self.mem_pool.pull_from_untracked(next_slice, false));

                            // update the starting index for the next iteration
                            index = index + usize::from(*byte);
                        }

                        let ctrl_desc = match self.create_indirect_ctrl(master, None, Some(&desc_lst)) {
                            Ok(desc) => desc,
                            Err(vq_err) => return Err(vq_err),
                        };
                        
                        Ok(TransferToken{
                            state: TransferState::Ready,
                            buff_tkn: BufferToken {
                                send_buff: None,
                                recv_buff: Some(Buffer::Indirect((ctrl_desc,desc_lst))),
                                vq: master,
                                ret_send: false,
                                ret_recv: false,
                                reusable: false,
                            },
                            vq: master,
                        })
                    },
                }
            },
            (Some((send_data, send_spec)), Some((recv_data, recv_spec))) => {
                match (send_spec, recv_spec) {
                    (BuffSpec::Single(send_size), BuffSpec::Single(recv_size)) => {
                        let send_data_slice = unsafe {(*send_data).as_slice_u8()};

                        // Buffer must have the right size
                        if send_data_slice.len() != send_size.into() {
                            return Err(VirtqError::BufferSizeWrong(send_data_slice.len()))
                        }

                        let send_desc = match self.mem_pool.pull_from(send_data_slice, false) {
                            Ok(desc) => desc,
                            Err(vq_err) => return Err(vq_err),
                        };

                        let recv_data_slice = unsafe {(*recv_data).as_slice_u8()};

                        // Buffer must have the right size
                        if recv_data_slice.len() != recv_size.into() {
                            return Err(VirtqError::BufferSizeWrong(recv_data_slice.len()))
                        }

                        let recv_desc = match self.mem_pool.pull_from(recv_data_slice, false) {
                            Ok(desc) => desc,
                            Err(vq_err) => return Err(vq_err),
                        };

                        Ok(TransferToken{
                            state: TransferState::Ready,
                            buff_tkn: BufferToken {
                                send_buff: Some(Buffer::Single(send_desc)),
                                recv_buff: Some(Buffer::Single(recv_desc)),
                                vq: master,
                                ret_send: false,
                                ret_recv: false,
                                reusable: false,
                            },
                            vq: master,
                        })
                    },
                    (BuffSpec::Single(send_size), BuffSpec::Multiple(recv_size_lst)) => {
                        let send_data_slice = unsafe {(*send_data).as_slice_u8()};

                        // Buffer must have the right size
                        if send_data_slice.len() != send_size.into() {
                            return Err(VirtqError::BufferSizeWrong(send_data_slice.len()))
                        }

                        let send_desc = match self.mem_pool.pull_from(send_data_slice, false) {
                            Ok(desc) => desc,
                            Err(vq_err) => return Err(vq_err),
                        };

                        let recv_data_slice = unsafe {(*recv_data).as_slice_u8()};
                        let mut recv_desc_lst: Vec<MemDescr> = Vec::with_capacity(recv_size_lst.len());
                        let mut index = 0usize;

                        for byte in recv_size_lst {
                            let end_index = index + usize::from(*byte);
                            let next_slice = match recv_data_slice.get(index..end_index){
                                Some(slice) => slice, 
                                None => return Err(VirtqError::BufferSizeWrong(recv_data_slice.len())),
                            };

                            match self.mem_pool.pull_from(next_slice, false) {
                                Ok(desc) => recv_desc_lst.push(desc),
                                Err(vq_err) => return Err(vq_err),
                            };

                            // update the starting index for the next iteration
                            index = index + usize::from(*byte);
                        }

                        Ok(TransferToken{
                            state: TransferState::Ready,
                            buff_tkn: BufferToken {
                                send_buff: Some(Buffer::Single(send_desc)),
                                recv_buff: Some(Buffer::Multiple(recv_desc_lst)),
                                vq: master,
                                ret_send: false,
                                ret_recv: false,
                                reusable: false,
                            },
                            vq: master,
                        })
                    },
                    (BuffSpec::Multiple(send_size_lst), BuffSpec::Multiple(recv_size_lst)) => {
                        let send_data_slice = unsafe {(*send_data).as_slice_u8()};
                        let mut send_desc_lst: Vec<MemDescr> = Vec::with_capacity(send_size_lst.len());
                        let mut index = 0usize;

                        for byte in send_size_lst {
                            let end_index = index + usize::from(*byte);
                            let next_slice = match send_data_slice.get(index..end_index){
                                Some(slice) => slice, 
                                None => return Err(VirtqError::BufferSizeWrong(send_data_slice.len())),
                            };

                            match self.mem_pool.pull_from(next_slice, false) {
                                Ok(desc) => send_desc_lst.push(desc),
                                Err(vq_err) => return Err(vq_err),
                            };

                            // update the starting index for the next iteration
                            index = index + usize::from(*byte);
                        }

                        let recv_data_slice = unsafe {(*recv_data).as_slice_u8()};
                        let mut recv_desc_lst: Vec<MemDescr> = Vec::with_capacity(recv_size_lst.len());
                        let mut index = 0usize;

                        for byte in recv_size_lst {
                            let end_index = index + usize::from(*byte);
                            let next_slice = match recv_data_slice.get(index..end_index){
                                Some(slice) => slice, 
                                None => return Err(VirtqError::BufferSizeWrong(recv_data_slice.len())),
                            };

                            match self.mem_pool.pull_from(next_slice, false) {
                                Ok(desc) => recv_desc_lst.push(desc),
                                Err(vq_err) => return Err(vq_err),
                            };

                            // update the starting index for the next iteration
                            index = index + usize::from(*byte);
                        }

                        Ok(TransferToken{
                            state: TransferState::Ready,
                            buff_tkn: BufferToken {
                                send_buff: Some(Buffer::Multiple(send_desc_lst)),
                                recv_buff: Some(Buffer::Multiple(recv_desc_lst)),
                                vq: master,
                                ret_send: false,
                                ret_recv: false,
                                reusable: false,
                            },
                            vq: master,
                        })
                    },
                    (BuffSpec::Multiple(send_size_lst), BuffSpec::Single(recv_size)) => {
                        let send_data_slice = unsafe {(*send_data).as_slice_u8()};
                        let mut send_desc_lst: Vec<MemDescr> = Vec::with_capacity(send_size_lst.len());
                        let mut index = 0usize;

                        for byte in send_size_lst {
                            let end_index = index + usize::from(*byte);
                            let next_slice = match send_data_slice.get(index..end_index){
                                Some(slice) => slice, 
                                None => return Err(VirtqError::BufferSizeWrong(send_data_slice.len())),
                            };

                            match self.mem_pool.pull_from(next_slice, false) {
                                Ok(desc) => send_desc_lst.push(desc),
                                Err(vq_err) => return Err(vq_err),
                            };

                            // update the starting index for the next iteration
                            index = index + usize::from(*byte);
                        }

                        let recv_data_slice = unsafe {(*recv_data).as_slice_u8()};

                        // Buffer must have the right size
                        if recv_data_slice.len() != recv_size.into() {
                            return Err(VirtqError::BufferSizeWrong(recv_data_slice.len()))
                        }

                        let recv_desc = match self.mem_pool.pull_from(recv_data_slice, false) {
                            Ok(desc) => desc,
                            Err(vq_err) => return Err(vq_err),
                        };

                        Ok(TransferToken{
                            state: TransferState::Ready,
                            buff_tkn: BufferToken {
                                send_buff: Some(Buffer::Multiple(send_desc_lst)),
                                recv_buff: Some(Buffer::Single(recv_desc)),
                                vq: master,
                                ret_send: false,
                                ret_recv: false,
                                reusable: false,
                            },
                            vq: master,
                        })
                    },
                    (BuffSpec::Indirect(send_size_lst), BuffSpec::Indirect(recv_size_lst)) => {
                        let send_data_slice = unsafe {(*send_data).as_slice_u8()};
                        let mut send_desc_lst: Vec<MemDescr> = Vec::with_capacity(send_size_lst.len());
                        let mut index = 0usize;

                        for byte in send_size_lst {
                            let end_index = index + usize::from(*byte);
                            let next_slice = match send_data_slice.get(index..end_index){
                                Some(slice) => slice, 
                                None => return Err(VirtqError::BufferSizeWrong(send_data_slice.len())),
                            };

                            send_desc_lst.push(self.mem_pool.pull_from_untracked(next_slice, false));

                            // update the starting index for the next iteration
                            index = index + usize::from(*byte);
                        }

                        let recv_data_slice = unsafe {(*recv_data).as_slice_u8()};
                        let mut recv_desc_lst: Vec<MemDescr> = Vec::with_capacity(recv_size_lst.len());
                        let mut index = 0usize;

                        for byte in recv_size_lst {
                            let end_index = index + usize::from(*byte);
                            let next_slice = match recv_data_slice.get(index..end_index){
                                Some(slice) => slice, 
                                None => return Err(VirtqError::BufferSizeWrong(recv_data_slice.len())),
                            };

                            recv_desc_lst.push(self.mem_pool.pull_from_untracked(next_slice, false));

                            // update the starting index for the next iteration
                            index = index + usize::from(*byte);
                        }

                        let ctrl_desc = match self.create_indirect_ctrl(master, Some(&send_desc_lst), Some(&recv_desc_lst)) {
                            Ok(desc) => desc,
                            Err(vq_err) => return Err(vq_err),
                        }; 

                        Ok(TransferToken{
                            state: TransferState::Ready,
                            buff_tkn: BufferToken {
                                recv_buff: Some(Buffer::Indirect((ctrl_desc.no_dealloc_clone(), recv_desc_lst))),
                                send_buff: Some(Buffer::Indirect((ctrl_desc, send_desc_lst))),
                                vq: master,
                                ret_send: false,
                                ret_recv: false,
                                reusable: false,
                            },
                            vq: master,
                        })
                    },
                    (BuffSpec::Indirect(_), BuffSpec::Single(_)) | (BuffSpec::Indirect(_), BuffSpec::Multiple(_)) => {
                        return Err(VirtqError::BufferInWithDirect)
                    },
                    (BuffSpec::Single(_), BuffSpec::Indirect(_)) | (BuffSpec::Multiple(_), BuffSpec::Indirect(_)) => {
                        return Err(VirtqError::BufferInWithDirect)
                    }
                }
            }
        } 
    }

    /// See `Virtq.prep_buffer()` documentation.
    pub fn prep_buffer<'b>(&self, master: &'b Virtq<'b>, send: Option<BuffSpec>, recv: Option<BuffSpec>) 
        -> Result<BufferToken<'b>, VirtqError> {
        match (send, recv) {
            // No buffers specified
            (None, None) => return Err(VirtqError::BufferNotSpecified),
            // Send buffer specified, No recv buffer
            (Some(spec), None) => {
                match spec {
                    BuffSpec::Single(size) => match self.mem_pool.pull(size) {
                        Ok(desc) => {
                            let buffer = Buffer::Single(desc);

                            Ok(BufferToken {
                                send_buff: Some(buffer),
                                recv_buff: None,
                                vq: master,
                                ret_send: true,
                                ret_recv: true,
                                reusable: true,
                            })
                        }
                        Err(vq_err) => return Err(vq_err),
                    },
                    BuffSpec::Multiple(size_lst) => {
                        let mut desc_lst: Vec<MemDescr> = Vec::with_capacity(size_lst.len());

                        for size in size_lst {
                            match self.mem_pool.pull(*size) {
                                Ok(desc) => desc_lst.push(desc),
                                Err(vq_err) => return Err(vq_err),
                            }
                        }

                        let buffer = Buffer::Multiple(desc_lst);

                        Ok(BufferToken{
                            send_buff: Some(buffer),
                            recv_buff: None,
                            vq: master,
                            ret_send: true,
                            ret_recv: true,
                            reusable: true,
                        })
                    },
                    BuffSpec::Indirect(size_lst) => {
                        let mut desc_lst: Vec<MemDescr> = Vec::with_capacity(size_lst.len());

                        for size in size_lst {
                            // As the indirect list does only consume one descriptor for the 
                            // control descriptor, the actual list is untracked
                            desc_lst.push(
                                self.mem_pool.pull_untracked(*size)
                            );
                        }

                        let ctrl_desc = match self.create_indirect_ctrl(master, Some(&desc_lst), None) {
                            Ok(desc) => desc,
                            Err(vq_err) => return Err(vq_err),
                        };
                        
                        let buffer = Buffer::Indirect((ctrl_desc, desc_lst));

                        Ok(BufferToken{
                            send_buff: Some(buffer),
                            recv_buff: None,
                            vq: master,
                            ret_send: true,
                            ret_recv: true,
                            reusable: true,
                        }) 
                    },
                }
            },
            // No send buffer, recv buffer is specified
            (None, Some(spec)) => {
                match spec {
                    BuffSpec::Single(size) => match self.mem_pool.pull(size) {
                        Ok(desc) => {
                            let buffer = Buffer::Single(desc);

                            Ok(BufferToken {
                                send_buff: None,
                                recv_buff: Some(buffer),
                                vq: master,
                                ret_send: true,
                                ret_recv: true,
                                reusable: true,
                            })
                        }
                        Err(vq_err) => return Err(vq_err),
                    },
                    BuffSpec::Multiple(size_lst) => {
                        let mut desc_lst: Vec<MemDescr> = Vec::with_capacity(size_lst.len());

                        for size in size_lst {
                            match self.mem_pool.pull(*size) {
                                Ok(desc) => desc_lst.push(desc),
                                Err(vq_err) => return Err(vq_err),
                            }
                        }

                        let buffer = Buffer::Multiple(desc_lst);

                        Ok(BufferToken{
                            send_buff: None,
                            recv_buff: Some(buffer),
                            vq: master,
                            ret_send: true,
                            ret_recv: true,
                            reusable: true,
                        })
                    },
                    BuffSpec::Indirect(size_lst) => {
                        let mut desc_lst: Vec<MemDescr> = Vec::with_capacity(size_lst.len());

                        for size in size_lst {
                            // As the indirect list does only consume one descriptor for the 
                            // control descriptor, the actual list is untracked
                            desc_lst.push(
                                self.mem_pool.pull_untracked(*size)
                            );
                        }

                        let ctrl_desc =  match self.create_indirect_ctrl(master, None, Some(&desc_lst)) {
                            Ok(desc) => desc,
                            Err(vq_err) => return Err(vq_err),
                        };
                        
                        let buffer = Buffer::Indirect((ctrl_desc, desc_lst));

                        Ok(BufferToken{
                            send_buff: None,
                            recv_buff: Some(buffer),
                            vq: master,
                            ret_send: true,
                            ret_recv: true,
                            reusable: true,
                        })
                    },
                }
            },
            // Send buffer specified, recv buffer specified
            (Some(send_spec), Some(recv_spec)) => {
                match (send_spec, recv_spec) {
                    (BuffSpec::Single(send_size), BuffSpec::Single(recv_size)) => {
                        let send_buff = match self.mem_pool.pull(send_size) {
                            Ok(desc) => {
                                Some(Buffer::Single(desc))
                            }
                            Err(vq_err) => return Err(vq_err),
                        };

                        let recv_buff = match self.mem_pool.pull(recv_size) {
                            Ok(desc) => {
                                Some(Buffer::Single(desc))
                            }
                            Err(vq_err) => return Err(vq_err),
                        };

                        Ok(BufferToken{
                            send_buff,
                            recv_buff,
                            vq: master,
                            ret_send: true,
                            ret_recv: true,
                            reusable: true,
                        })
                    },
                    (BuffSpec::Single(send_size), BuffSpec::Multiple(recv_size_lst)) => {
                        let send_buff = match self.mem_pool.pull(send_size) {
                            Ok(desc) => {
                                Some(Buffer::Single(desc))
                            }
                            Err(vq_err) => return Err(vq_err),
                        };

                        let mut desc_lst: Vec<MemDescr> = Vec::with_capacity(recv_size_lst.len());

                        for size in recv_size_lst {
                            match self.mem_pool.pull(*size) {
                                Ok(desc) => desc_lst.push(desc),
                                Err(vq_err) => return Err(vq_err),
                            }
                        }

                        let recv_buff = Some(Buffer::Multiple(desc_lst));

                        Ok(BufferToken{
                            send_buff,
                            recv_buff,
                            vq: master,
                            ret_send: true,
                            ret_recv: true,
                            reusable: true,
                        })

                    },
                    (BuffSpec::Multiple(send_size_lst), BuffSpec::Multiple(recv_size_lst)) => {
                        let mut send_desc_lst: Vec<MemDescr> = Vec::with_capacity(send_size_lst.len());

                        for size in send_size_lst {
                            match self.mem_pool.pull(*size) {
                                Ok(desc) => send_desc_lst.push(desc),
                                Err(vq_err) => return Err(vq_err),
                            }
                        }

                        let send_buff = Some(Buffer::Multiple(send_desc_lst));

                        let mut recv_desc_lst: Vec<MemDescr> = Vec::with_capacity(recv_size_lst.len());

                        for size in recv_size_lst {
                            match self.mem_pool.pull(*size) {
                                Ok(desc) => recv_desc_lst.push(desc),
                                Err(vq_err) => return Err(vq_err),
                            }
                        }

                        let recv_buff = Some(Buffer::Multiple(recv_desc_lst));

                        Ok(BufferToken{
                            send_buff,
                            recv_buff,
                            vq: master,
                            ret_send: true,
                            ret_recv: true,
                            reusable: true,
                        })
                    },
                    (BuffSpec::Multiple(send_size_lst), BuffSpec::Single(recv_size)) => {
                        let mut send_desc_lst: Vec<MemDescr> = Vec::with_capacity(send_size_lst.len());

                        for size in send_size_lst {
                            match self.mem_pool.pull(*size) {
                                Ok(desc) => send_desc_lst.push(desc),
                                Err(vq_err) => return Err(vq_err),
                            }
                        }

                        let send_buff = Some(Buffer::Multiple(send_desc_lst));

                        let recv_buff = match self.mem_pool.pull(recv_size) {
                            Ok(desc) => {
                                Some(Buffer::Single(desc))
                            }
                            Err(vq_err) => return Err(vq_err),
                        };

                        Ok(BufferToken{
                            send_buff,
                            recv_buff,
                            vq: master,
                            ret_send: true,
                            ret_recv: true,
                            reusable: true,
                        })
                    },
                    (BuffSpec::Indirect(send_size_lst), BuffSpec::Indirect(recv_size_lst)) => {
                        let mut send_desc_lst: Vec<MemDescr> = Vec::with_capacity(send_size_lst.len());

                        for size in send_size_lst {
                            // As the indirect list does only consume one descriptor for the 
                            // control descriptor, the actual list is untracked
                            send_desc_lst.push(
                                self.mem_pool.pull_untracked(*size)
                            );
                        }

                        let mut recv_desc_lst: Vec<MemDescr> = Vec::with_capacity(recv_size_lst.len());

                        for size in recv_size_lst {
                            // As the indirect list does only consume one descriptor for the 
                            // control descriptor, the actual list is untracked
                            recv_desc_lst.push(
                                self.mem_pool.pull_untracked(*size)
                            );
                        }

                        let ctrl_desc =  match self.create_indirect_ctrl(master, Some(&send_desc_lst), Some(&recv_desc_lst)) {
                            Ok(desc) => desc,
                            Err(vq_err) => return Err(vq_err),
                        };
                        
                        let recv_buff = Some(Buffer::Indirect((ctrl_desc.no_dealloc_clone(), recv_desc_lst)));
                        let send_buff = Some(Buffer::Indirect((ctrl_desc, send_desc_lst)));

                        Ok(BufferToken{
                            send_buff,
                            recv_buff,
                            vq: master,
                            ret_send: true,
                            ret_recv: true,
                            reusable: true,
                        })
                    },
                    (BuffSpec::Indirect(_), BuffSpec::Single(_)) | (BuffSpec::Indirect(_), BuffSpec::Multiple(_)) => {
                        return Err(VirtqError::BufferInWithDirect)
                    },
                    (BuffSpec::Single(_), BuffSpec::Indirect(_)) | (BuffSpec::Multiple(_), BuffSpec::Indirect(_)) => {
                        return Err(VirtqError::BufferInWithDirect)
                    }
                }
            },
        }
    }

    pub fn size(&self) -> VqSize {
        VqSize(self.size)
    }
}

// Private Interface for PackedVq
impl<'a> PackedVq<'a> {
    fn create_indirect_ctrl<'b>(&self, vq: &'b Virtq, send: Option<&Vec<MemDescr>>, recv: Option<&Vec<MemDescr>>) -> Result<MemDescr<'b>, VirtqError>{
        // Need to match (send, recv) twice, as the "size" of the control descriptor to be pulled must be known in advance.
        let mut len: usize;
        match (send, recv) {
            (None, None) => return Err(VirtqError::BufferNotSpecified),
            (None, Some(recv_desc_lst)) => {
                len = recv_desc_lst.len();
            },
            (Some(send_desc_lst), None) => {
                len = send_desc_lst.len();
            },
            (Some(send_desc_lst), Some(recv_desc_lst)) => {
                len = send_desc_lst.len() + recv_desc_lst.len();
            },
        }

        let sz_indrct_lst = Bytes(core::mem::size_of::<Descriptor>() * len);
        let mut ctrl_desc = match self.mem_pool.pull(sz_indrct_lst) {
            Ok(desc) => desc,
            Err(vq_err) => return Err(vq_err),
        };

        // For indexing into the allocated memory area. This reduces the 
        // function to only iterate over the MemDescr once and not twice
        // as otherwise needed if the raw descriptor bytes were to be stored
        // in an array.
        let mut crtl_desc_iter = 0usize;

        match (send, recv) {
            (None, None) => return Err(VirtqError::BufferNotSpecified),
            // Only recving descriptorsn (those are writabel by device)
            (None, Some(recv_desc_lst)) => {
                for desc in recv_desc_lst {
                   let raw: [u8; 16] = Descriptor::new(
                        (desc.ptr as u64),
                        (desc.len as u32),
                        0,
                        DescrFlags::VIRTQ_DESC_F_WRITE.into()
                   ).to_le_bytes();
                   
                   for byte in 0..16 {
                       ctrl_desc[crtl_desc_iter] = raw[byte];
                       crtl_desc_iter += 1;
                   }
                }
                Ok(ctrl_desc)
            },
            // Only sending descritpors
            (Some(send_desc_lst), None) => {
                for desc in send_desc_lst {
                    let raw: [u8; 16] = Descriptor::new(
                        (desc.ptr as u64),
                        (desc.len as u32),
                        0,
                        0, 
                   ).to_le_bytes();
                   
                   for byte in 0..16 {
                       ctrl_desc[crtl_desc_iter] = raw[byte];
                       crtl_desc_iter += 1;
                   }
                }
                Ok(ctrl_desc)
            },
            (Some(send_desc_lst), Some(recv_desc_lst)) => {
                // Send descriptors ALWAYS before receiving ones.
                for desc in send_desc_lst {
                    let raw: [u8; 16] = Descriptor::new(
                        (desc.ptr as u64),
                        (desc.len as u32),
                        0,
                        0, 
                   ).to_le_bytes();
                   
                   for byte in 0..16 {
                       ctrl_desc[crtl_desc_iter] = raw[byte];
                       crtl_desc_iter += 1;
                   }
                }

                for desc in recv_desc_lst {
                    let raw: [u8; 16] = Descriptor::new(
                        (desc.ptr as u64),
                        (desc.len as u32),
                        0,
                        DescrFlags::VIRTQ_DESC_F_WRITE.into()
                   ).to_le_bytes();
                   
                   for byte in 0..16 {
                       ctrl_desc[crtl_desc_iter] = raw[byte];
                       crtl_desc_iter += 1;
                   }
                }

                Ok(ctrl_desc)
            },
        }
    }
}

impl<'a> Drop for PackedVq<'a> {
    fn drop(&mut self) {
        todo!("rerutn leaked memory and ensure deallocation")
    }
}

pub mod error {
    pub enum VqPackedError {
        General,
        SizeNotAllowed(u16),
        QueueNotExisting(u16)
    }
}