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
//! Drivers who need to a more fine grained access to the specifc queues must
//! use the respective virtqueue structs directly.
pub mod packed;
pub mod split;

use self::packed::PackedVq;
use self::split::SplitVq;
use self::split::VqSize as SplitVqSize;
use self::packed::VqSize as PackedVqSize;

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




/// The Virtqueu interface is implemented by both virtqueue types
/// [PackedVq](structs.PackedVq.html) and [SplitVq](structs.SplitVq.html)
/// in order to ensure a common interface.
pub trait VqInterface {
    type VqSize;

    fn get_size(&self) -> Self::VqSize;
}
