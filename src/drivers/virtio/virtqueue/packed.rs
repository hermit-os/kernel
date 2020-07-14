// Copyright (c) 2020 Frederik Schulz, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! This module contains Virtio's packed virtqueue. 
//! See Virito specification v1.1. - 2.7
use alloc::vec::Vec;

use drivers::virtio::virtqueue::VqInterface;

/// A newtype of bool used for convenience in context with 
/// packed queues wrap counter.
///
/// For more details see Virtio specification v1.1. - 2.7.1
#[derive(Copy, Clone, Debug)]
pub struct WrapCount(bool);

impl WrapCount {
    /// Toogles a given wrap count to respectiver other value.
    ///
    /// If WrapCount(true) returns WrapCount(false), 
    /// if WrapCount(false) returns WrapCount(true).
    pub fn wrap(&self) {
        unimplemented!();
    }
}

impl From<bool> for WrapCount {
    /// Creates a wrap count from an boolean input. 
    /// Should always be used to enhance readablilty of code,
    /// as booleans are misleading when used in "counter" context.
    fn from(val: bool) -> Self {
        if val {
            WrapCount(true)
        } else {
            WrapCount(false)
        }
    }
}

/// A newtype of a u16, ensuring the maximum size of a packed queues is not 
/// exceeded.
#[derive(Copy, Clone, Debug)]
pub struct VqSize(u16);

impl From<u16> for VqSize {
    /// Creates a newtype of a u16, but ensures the maximum value is
    /// 2^15 = 32768. In order to ensure packed virtqueues functionality
    /// VqSize instances MUST ALWAYS be create via new function.
    fn from(val: u16) -> Self {
       // Assure size of queue is smaller than 2^15 = 32768.
        if val < 32768 {
            VqSize(val)
        } else {
            warn!("Packed vqueue size = {}, is above maximum. Reducing to maximum value.", val);
            VqSize(32768u16)
        } 
    }
}

/// A newtype of a u16, ensuring the maximum id of a descriptor in a packed vq is not
/// exceeded
#[derive(Copy, Clone, Debug)]
struct BuffId(u16);

impl From<u16> for BuffId {
    fn from(val: u16) -> Self {
        // Assure size of queue is smaller than 2^15 = 32768.
        if val < 32768 {
            BuffId(val)
        } else {
            warn!("Packed vqueue size = {}, is above maximum. Reducing to maximum value.", val);
            BuffId(32768u16)
        }
    } 
} 


/// An ID pool for buffers
struct BuffIdPool {
    pool: Vec<BuffId>
}

impl BuffIdPool {
    /// Creates an instance of BuffIdPool.
    /// The pool will hold as many IDs as the queue size indicates.
    pub fn new(queue_size: VqSize) -> Self {
       unimplemented!();
    }

    /// Returns an [BuffId](structs.BuffId.html) wrapped in an Option.
    /// Returns None if ID pool is exhausted.
    pub fn get_id() -> BuffId {
        unimplemented!();
    }

    /// Returns an ID from a buffer after it has been used. 
    /// Should never be called driectly. Instead it is called by 
    /// virtqueue structure upon dropping a buffer.
    fn return_id(id: BuffId) {
        unimplemented!();
    }
}

/// Decriptor ring used in packed virtqueues.
///
/// Alignment see Virtio specification v1.1. - 2.7.10.1
#[repr(C, align(16))]
struct DescriptorRing {
    ring: Vec<Descriptor>,
}


#[repr(C)]
struct Descriptor {
    address: u64,
    len: u32,
    buff_id: BuffId,
    flags: u16,
}

impl Descriptor {
    pub fn mark_used(&self) {
        unimplemented!();
    }

    pub fn mark_avail(&self) {
        unimplemented!();
    }

    pub fn is_used() {
        unimplemented!();
    }

    pub fn is_avail() {
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
    /// Enables notifications by setting the LSB.
    /// See Virito specification v1.1. - 2.7.10
    pub fn enable_notif() {
        unimplemented!();
    }

    /// Disables notifications by unsetting the LSB.
    /// See Virtio specification v1.1. - 2.7.10
    pub fn disable_notif() {
        unimplemented!();
    }

    /// Reads notification bit (i.e. LSB) and returns value.
    /// If notifications are enabled returns true, else false.
    pub fn is_notif() -> bool {
        unimplemented!();
    }


    pub fn enable_specific(descriptor_id: u16, on_count: WrapCount) {
        // Check if VIRTIO_F_RING_EVENT_IDX has been negotiated

        // Check if descriptor_id is below 2^15

        // Set second bit from LSB to true

        // Set descriptor id, triggering notification

        // Set which wrap counter triggers

        unimplemented!();
    }
}


/// Virtio's packed virtqueue structure.
/// See Virtio Specification 2.7.
#[repr(C)]
struct PackedVqRaw {
    descriptor_ring: DescriptorRing,
    device_event: EventSuppr,
    driver_event: EventSuppr,
}

/// A wrapper struct of the actual packed virtqueue specified in the standard
/// 
pub struct PackedVq {
    vqueue: PackedVqRaw,
    size: VqSize,
    wrap_count: WrapCount,
}

impl PackedVq {
    pub fn new(size: VqSize) -> Self {
        unimplemented!();
    }
}

impl VqInterface for PackedVq {
    type VqSize =  VqSize;

    fn get_size(&self) -> Self::VqSize {
        self.size
    }
}