// Copyright (c) 2020 Frederik Schulz, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! This module contains Virtio's split virtqueue. 
//! See Virito specification v1.1. - 2.6 

use drivers::virtio::virtqueue::VqInterface;

#[derive(Copy, Clone, Debug)]
pub struct VqSize(u32);

impl From<u32> for VqSize {
    fn from(val: u32) -> Self {
        VqSize(val)
    }
}

struct SplitVqRaw {

}  

/// Virtio's split virtqueue structure
pub struct SplitVq {
    queue: SplitVqRaw,
    size: VqSize,
}

impl SplitVq {
    pub fn new(size: VqSize) -> Self {
        unimplemented!();
    }
}

impl VqInterface for SplitVq {
    type VqSize = VqSize;

    fn get_size(&self) -> Self::VqSize {
        self.size
    }
}