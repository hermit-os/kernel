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
pub mod env;

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
    /// Big endian UNSIGNED 16-bit integer.
    ///
    /// In order to ensure right endianess MUST
    /// use construction via from(u16).
    /// # Example
    /// 
    /// ```
    /// let number = u16;
    /// // Creates an big endian u16 integer
    /// let be16 = Be16::from(number);
    ///
    /// // WARN: Creates an u16 depending on endianess of system!
    /// let os_u16 = Be16(number)
    /// ```
    #[derive(Copy, Clone, Debug)]
    pub struct Be16(pub u16);

    impl From<u16> for Be16 {
        fn from(val: u16) -> Self {
            Be16(val.to_be())
        }
    }
    /// Big endian unsigned 32-bit integer.
    ///
    /// In order to ensure right endianess MUST
    /// use construction via from(u32).
    /// # Example
    /// 
    /// ```
    /// let number = u32;
    /// // Creates an big endian u32 integer
    /// let be32 = Be32::from(number);
    ///
    /// // WARN: Creates an u32 depending on endianess of system!
    /// let os_u32 = Be32(number)
    /// ```
    #[derive(Copy, Clone, Debug)]
    pub struct Be32(pub u32);

    impl From<u32> for Be32 {
        fn from(val: u32) -> Self {
            Be32(val.to_be())
        }
    }
    /// Big endian unsigned 64-bit integer.
    ///
    /// In order to ensure right endianess MUST
    /// use construction via from(u64).
    /// # Example
    /// 
    /// ```
    /// let number = u64;
    /// // Creates an big endian u64 integer
    /// let be64 = Be64::from(number);
    ///
    /// // WARN: Creates an u64 depending on endianess of system!
    /// let os_u64 = Be64(number)
    /// ```
    #[derive(Copy, Clone, Debug)]
    pub struct Be64(pub u64);

    impl From<u64> for Be64 {
        fn from(val: u64) -> Self {
            Be64(val.to_be())
        }
    }
    /// Little endian unsigned 16-bit integer.
    ///
    /// In order to ensure right endianess MUST
    /// use construction via from(u16).
    /// # Example
    /// 
    /// ```
    /// let number = u16;
    /// // Creates an little endian u16 integer
    /// let le16 = Le16::from(number);
    ///
    /// // WARN: Creates an u16 depending on endianess of system!
    /// let os_u16 = Le16(number)
    /// ```
    #[derive(Copy, Clone, Debug)]
    pub struct Le16(pub u16);

    impl From<u16> for Le16 {
        fn from(val: u16) -> Self {
            Le16(val.to_le())
        }
    }
    /// Little endian unsigned 32-bit integer.
    ///
    /// In order to ensure right endianess MUST
    /// use construction via from(u32).
    /// # Example
    /// 
    /// ```
    /// let number = u32;
    /// // Creates an little endian u32 integer
    /// let le32 = Le32::from(number);
    ///
    /// // WARN: Creates an u32 depending on endianess of system!
    /// let os_u32 = Le32(number)
    /// ```
    #[derive(Copy, Clone, Debug)]
    pub struct Le32(pub u32);

    impl From<u32> for Le32 {
        fn from(val: u32) -> Self {
            Le32(val.to_le())
        }
    }
    /// Little endian unsigned 64-bit integer.
    ///
    /// In order to ensure right endianess MUST
    /// use construction via from(u64).
    /// # Example
    /// 
    /// ```
    /// let number = u64;
    /// // Creates an little endian u64 integer
    /// let le64 = Le64::from(number);
    ///
    /// // WARN: Creates an u64 depending on endianess of system!
    /// let os_u64 = Le64(number)
    /// ```
    #[derive(Copy, Clone, Debug)]
    pub struct Le64(pub u64);

    impl From<u64> for Le64 {
        fn from(val: u64) -> Self {
            Le64(val.to_le())
        }
    }
}

/// A module containing Virtio's feature bits.
pub mod features {
    /// Virtio's feature bits inside an enum. 
    /// See Virtio specification v1.1. - 6
    #[allow(dead_code, non_camel_case_types)]
    #[derive(Clone, Copy, Debug)]
    #[repr(u64)]
    pub enum Features {
        VIRTIO_F_RING_INDIRECT_DESC = 1 << 28,
	    VIRTIO_F_RING_EVENT_IDX = 1 << 29,
	    VIRTIO_F_VERSION_1 = 1 << 32,
	    VIRTIO_F_ACCESS_PLATFORM = 1 << 33,
	    VIRTIO_F_RING_PACKED = 1 << 34,
	    VIRTIO_F_IN_ORDER = 1 << 35,
	    VIRTIO_F_ORDER_PLATFORM = 1 << 36,
	    VIRTIO_F_SR_IOV = 1 << 37,
	    VIRTIO_F_NOTIFICATION_DATA = 1 << 38,
    }
}


/// A module containing virtios driver trait.
/// 
/// The module contains ...
pub mod driver {
    pub trait VirtioDriver {
        type Cfg;
        fn map_cfg(&self) -> Self::Cfg;
        fn add_buff(&self);
        fn get_buff(&self);
        fn process_buff(&self);
        fn set_notif(&self);
    }
}