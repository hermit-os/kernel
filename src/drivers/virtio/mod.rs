// Copyright (c) 2020 Frederik Schulz, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! A module containing virtios core infrastructure for hermit-rs.
//! 
//! The module contains virtios transport mechanisms, virtqueues and virtio specific errors

pub mod depr;
pub mod transport;
pub mod virtqueue;
pub mod driver;

pub mod error {
    use core::fmt;
    #[derive(Debug)]
    pub enum VirtioError {
        DriverFail,
        DevNotSupported(u16),
    }

    impl fmt::Display for VirtioError {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            match *self {
                VirtioError::DriverFail => write!(f, "Driver failed to init devic."),
                VirtioError::DevNotSupported(id) => write!(f, "Devie with id 0x{:x} not supported.", id)
            }  
        }
    }
}


/// A module containing virtios new types and corresponding convenient functions.
///
/// The module contains little- and big-endian types of unsignend integers. The 
/// terminology follow the virtio spec. v1.1 - 1.4
pub mod types {
    /// Big endian unsigned 16-bit integer.
    pub struct Be16(pub u16);
    /// Big endian unsigned 32-bit integer.
    pub struct Be32(pub u32);
    /// Big endian unsigned 64-bit integer.
    pub struct Be64(pub u64);

    /// Little endian unsigned 16-bit integer.
    pub struct Le16(pub u16);
    /// Little endian unsigned 32-bit integer.
    pub struct Le32(pub u32);
    /// Little endian unsigned 64-bit integer.
    pub struct Le64(pub u64);
}