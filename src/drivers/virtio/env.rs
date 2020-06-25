// Copyright (c) 2020 Frederik Schulz, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! A module containing all environment specific funtion calls.
//! 
//! The module should easy partability of the code. But its main aspect is to 
//! ensure a single location needs changes, in cases where the fundamental kernel code is changed.

#[derive(Copy, Clone, Debug)]
pub enum MemAddr{
    Bit32(u32),
    Bit64(u64),
}

impl From<u32> for MemAddr {
    fn from(addr: u32) -> Self {
        MemAddr::Bit32(addr)
    }
}

impl From<u64> for MemAddr {
    fn from(addr: u64) -> Self {
        MemAddr::Bit64(addr)
    }
}

pub struct VirtMemAddr(usize);

impl From<u32> for VirtMemAddr {
    fn from(addr: u32) -> Self {
        unimplemented!();
        // TODO: check if current system is 32 bit, then okay. else fail
    }
}

impl From<u64> for VirtMemAddr {
    fn from(addr: u64) -> Self {
        unimplemented!();
        // TODO: check if current system is 64 bit, then okaym ekse fail
    }
}

pub struct PhyMemAddr(usize);

impl From<u32> for PhyMemAddr {
    fn from(addr: u32) -> Self {
        unimplemented!();
        // TODO: check if current system is 32 bit, then okay. else fail
    }
}

impl From<u64> for PhyMemAddr {
    fn from(addr: u64) -> Self {
        unimplemented!();
        // TODO: check if current system is 64 bit, then okaym ekse fail
    }
}
