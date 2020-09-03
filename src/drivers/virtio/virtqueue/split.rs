// Copyright (c) 2020 Frederik Schulz, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! This module contains Virtio's split virtqueue. 
//! See Virito specification v1.1. - 2.6 

use super::VqSize;

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

    pub fn size(&self) -> VqSize {
        todo!("implement size() for split queue")
    }
}