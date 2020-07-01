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
    use arch::x86_64::kernel::pci::error::PciError;

    #[derive(Debug)]
    pub enum VirtioError {
        FromPci(PciError),
        DevNotSupported(u16),
    }

    impl fmt::Display for VirtioError {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            match self {
                VirtioError::FromPci(pci_error) => match pci_error {
                    PciError::General(id) => write!(f, "Driver failed to initalize device with id: 0x{:x}. Due to unknown reasosn!", id),
                    PciError::NoBar(id ) => write!(f, "Driver failed to initalize device with id: 0x{:x}. Reason: No BAR's found.", id), 
                    PciError::NoCapPtr(id) => write!(f, "Driver failed to initalize device with id: 0x{:x}. Reason: No Capabilites pointer found.", id),
                    PciError::BadCapPtr(id) => write!(f, "Driver failed to initalize device with id: 0x{:x}. Reason: Malformed Capabilites pointer.", id),
                    PciError::NoBarForCap(id) => write!(f, "Driver failed to initalize device with id: 0x{:x}. Reason: Bar indicated by capability not found.", id),
                },
                VirtioError::DevNotSupported(id) => write!(f, "Devie with id 0x{:x} not supported.", id)
            }  
        }
    }
}


/// A module containing virtios new types and corresponding convenient functions.
///
/// The module contains little- and big-endian types of unsignend integers. The 
/// terminology follow the virtio spec. v1.1 - 1.4
///
/// INFO: Currently RustyHermit only supports little endian. Little endian 
/// types are still used in order to indicate, where endianess is important
/// in case the system is ported.
pub mod types {
    use core::ops::{Add, Sub, Mul, BitAnd, BitAndAssign, BitXor, BitXorAssign, BitOr, BitOrAssign};

    /// Native endian u16 indicating this value represents a big endian u16.
    /// The value is not stored as a big endian u16 inside the struct in order to 
    /// allow correct math operations without transformating it all the time into the 
    /// native endianess of the machine.
    ///
    /// In order to receive a big endian coded u16 one must use the 
    /// [as_u16()](Be16::as_u16) function. Native endian coded u16 can be retrieved via the
    /// [to_ne16()](Be16::to_ne_u16) function.
    #[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
    pub struct Be16(u16);

    impl Be16 {
        /// Returns the wrapped u16 as a big endian coded u16.
        pub fn as_be(self) -> u16 {
            self.0.to_be()
        }

        /// Returns the wrapped u16, which is native endian coded!
        pub fn as_ne(self) -> u16 {
            self.0
        }
    }

    impl From<u16> for Be16 {
        fn from(val: u16) -> Be16 {
            Be16(val)
        }
    }

    impl Add for Be16 {
        type Output = Be16;

        fn add(self, other: Self ) -> Self::Output {
            Be16(self.0 + other.0)
        }
    }

    impl Sub for Be16 {
        type Output = Be16;

        fn sub(self, other: Self) -> Self::Output {
            Be16(self.0 - other.0)
        }
    }

    impl Mul for Be16 {
        type Output = Be16;

        fn mul(self, rhs: Self) -> Self::Output {
            Be16(self.0 * rhs.0)
        }
    }

    impl BitAnd for Be16 {
        type Output = Be16;
        
        fn bitand(self, rhs: Be16) -> Self::Output {
            Be16(self.0 & rhs.0)
        }
    }

    impl BitAndAssign for Be16 {
        fn bitand_assign(&mut self, rhs: Be16) {
            *self = Be16(self.0 & rhs.0)
        }
    }

    impl BitOr for Be16 {
        type Output = Be16;

        fn bitor(self, rhs: Be16) -> Self::Output {
            Be16(self.0 | rhs.0)
        }
    }

    impl BitOrAssign for Be16 {
        fn bitor_assign(&mut self, rhs: Be16) {
            *self = Be16(self.0 | rhs.0) 
        }
    }

    impl BitXor for Be16 {
        type Output = Be16;

        fn bitxor(self, rhs: Be16) -> Self::Output {
            Be16(self.0 ^ rhs.0)
        }
    }

    impl BitXorAssign for Be16 {
        fn bitxor_assign(&mut self, rhs: Be16) {
           *self = Be16(self.0 ^ rhs.0) 
        }
    }

    /// Native endian u32 indicating this value represents a big endian u32.
    /// The value is not stored as a big endian u32 inside the struct in order to 
    /// allow correct math operations without transformating it all the time into the 
    /// native endianess of the machine.
    ///
    /// In order to receive a big endian coded u32 one must use the 
    /// [as_u32()](Be32::as_u32) function. Native endian coded u32 can be retrieved via the
    /// [to_ne32()](Be32::to_ne_u32) function.
    #[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
    pub struct Be32(u32);

    impl Be32 {
        /// Returns the wrapped u32 as a little endian coded u32.
        pub fn as_be(self) -> u32 {
            self.0.to_be()
        }

        /// Returns the wrapped u32, which is native endian coded!
        pub fn as_ne(self) -> u32 {
            self.0
        }
    }

    impl From<u32> for Be32 {
        fn from(val: u32) -> Be32 {
            Be32(val)
        }
    }

    impl Add for Be32 {
        type Output = Be32;

        fn add(self, other: Self ) -> Self::Output {
            Be32(self.0 + other.0)
        }
    }

    impl Sub for Be32 {
        type Output = Be32;

        fn sub(self, other: Self) -> Self::Output {
            Be32(self.0 - other.0)
        }
    }

    impl Mul for Be32 {
        type Output = Be32;

        fn mul(self, rhs: Self) -> Self::Output {
            Be32(self.0 * rhs.0)
        }
    }

    impl BitAnd for Be32 {
        type Output = Be32;
        
        fn bitand(self, rhs: Be32) -> Self::Output {
            Be32(self.0 & rhs.0)
        }
    }

    impl BitAndAssign for Be32 {
        fn bitand_assign(&mut self, rhs: Be32) {
            *self = Be32(self.0 & rhs.0)
        }
    }

    impl BitOr for Be32 {
        type Output = Be32;

        fn bitor(self, rhs: Be32) -> Self::Output {
            Be32(self.0 | rhs.0)
        }
    }

    impl BitOrAssign for Be32 {
        fn bitor_assign(&mut self, rhs: Be32) {
            *self = Be32(self.0 | rhs.0) 
        }
    }

    impl BitXor for Be32 {
        type Output = Be32;

        fn bitxor(self, rhs: Be32) -> Self::Output {
            Be32(self.0 ^ rhs.0)
        }
    }

    impl BitXorAssign for Be32 {
        fn bitxor_assign(&mut self, rhs: Be32) {
           *self = Be32(self.0 ^ rhs.0) 
        }
    }

    /// Native endian u64 indicating this value represents a big endian u64.
    /// The value is not stored as a big endian u64 inside the struct in order to 
    /// allow correct math operations without transformating it all the time into the 
    /// native endianess of the machine.
    ///
    /// In order to receive a big endian coded u64 one must use the 
    /// [as_u64()](Be64::as_u64) function. Native endian coded u64 can be retrieved via the
    /// [to_ne64()](Be64::to_ne_u64) function.
    #[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
    pub struct Be64(u64);

    impl Be64 {
        /// Returns the wrapped u64 as a little endian coded u64.
        pub fn as_be(self) -> u64 {
            self.0.to_be()
        }

        /// Returns the wrapped u64, which is native endian coded!
        pub fn as_ne(self) -> u64 {
            self.0
        }
    }

    impl From<u64> for Be64 {
        fn from(val: u64) -> Be64 {
            Be64(val)
        }
    }

    impl Add for Be64 {
        type Output = Be64;

        fn add(self, other: Self ) -> Self::Output {
            Be64(self.0 + other.0)
        }
    }

    impl Sub for Be64 {
        type Output = Be64;

        fn sub(self, other: Self) -> Self::Output {
            Be64(self.0 - other.0)
        }
    }

    impl Mul for Be64 {
        type Output = Be64;

        fn mul(self, rhs: Self) -> Self::Output {
            Be64(self.0 * rhs.0)
        }
    }

    impl BitAnd for Be64 {
        type Output = Be64;
        
        fn bitand(self, rhs: Be64) -> Self::Output {
            Be64(self.0 & rhs.0)
        }
    }

    impl BitAndAssign for Be64 {
        fn bitand_assign(&mut self, rhs: Be64) {
            *self = Be64(self.0 & rhs.0)
        }
    }

    impl BitOr for Be64 {
        type Output = Be64;

        fn bitor(self, rhs: Be64) -> Self::Output {
            Be64(self.0 | rhs.0)
        }
    }

    impl BitOrAssign for Be64 {
        fn bitor_assign(&mut self, rhs: Be64) {
            *self = Be64(self.0 | rhs.0) 
        }
    }

    impl BitXor for Be64 {
        type Output = Be64;

        fn bitxor(self, rhs: Be64) -> Self::Output {
            Be64(self.0 ^ rhs.0)
        }
    }

    impl BitXorAssign for Be64 {
        fn bitxor_assign(&mut self, rhs: Be64) {
           *self = Be64(self.0 ^ rhs.0) 
        }
    }


    /// Native endian u16 indicating this value represents a little endian u16.
    /// The value is not stored as a little endian u16 inside the struct in order to 
    /// allow correct math operations without transformating it all the time into the 
    /// native endianess of the machine.
    ///
    /// In order to receive a little endian coded u16 one must use the 
    /// [as_u16()](Le16::as_u16) function. Native endian coded u16 can be retrieved via the
    /// [to_ne16()](Le16::to_ne_u16) function.
    #[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
    pub struct Le16(u16);

    impl Le16 {
        /// Returns the wrapped u16 as a little endian coded u16.
        pub fn as_le(self) -> u16 {
            self.0.to_le()
        }

        /// Returns the wrapped u16, which is native endian coded!
        pub fn as_ne(self) -> u16 {
            self.0
        }
    }

    impl From<u16> for Le16 {
        fn from(val: u16) -> Le16 {
            Le16(val)
        }
    }

    impl Add for Le16 {
        type Output = Le16;

        fn add(self, other: Self ) -> Self::Output {
            Le16(self.0 + other.0)
        }
    }

    impl Sub for Le16 {
        type Output = Le16;

        fn sub(self, other: Self) -> Self::Output {
            Le16(self.0 - other.0)
        }
    }

    impl Mul for Le16 {
        type Output = Le16;

        fn mul(self, rhs: Self) -> Self::Output {
            Le16(self.0 * rhs.0)
        }
    }

    impl BitAnd for Le16 {
        type Output = Le16;
        
        fn bitand(self, rhs: Le16) -> Self::Output {
            Le16(self.0 & rhs.0)
        }
    }

    impl BitAndAssign for Le16 {
        fn bitand_assign(&mut self, rhs: Le16) {
            *self = Le16(self.0 & rhs.0)
        }
    }

    impl BitOr for Le16 {
        type Output = Le16;

        fn bitor(self, rhs: Le16) -> Self::Output {
            Le16(self.0 | rhs.0)
        }
    }

    impl BitOrAssign for Le16 {
        fn bitor_assign(&mut self, rhs: Le16) {
            *self = Le16(self.0 | rhs.0) 
        }
    }

    impl BitXor for Le16 {
        type Output = Le16;

        fn bitxor(self, rhs: Le16) -> Self::Output {
            Le16(self.0 ^ rhs.0)
        }
    }

    impl BitXorAssign for Le16 {
        fn bitxor_assign(&mut self, rhs: Le16) {
           *self = Le16(self.0 ^ rhs.0) 
        }
    }


    /// Native endian u32 indicating this value represents a little endian u32.
    /// The value is not stored as a little endian u32 inside the struct in order to 
    /// allow correct math operations without transformating it all the time into the 
    /// native endianess of the machine.
    ///
    /// In order to receive a little endian coded u32 one must use the 
    /// [as_u32()](Le32::as_u32) function. Native endian coded u32 can be retrieved via the
    /// [to_ne32()](Le32::to_ne_u32) function.
    #[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
    pub struct Le32(u32);

    impl Le32 {
        /// Returns the wrapped u32 as a little endian coded u32.
        pub fn as_le(self) -> u32 {
            self.0.to_le()
        }

        /// Returns the wrapped u32, which is native endian coded!
        pub fn as_ne(self) -> u32 {
            self.0
        }
    }

    impl From<u32> for Le32 {
        fn from(val: u32) -> Le32 {
            Le32(val)
        }
    }

    impl Add for Le32 {
        type Output = Le32;

        fn add(self, other: Self ) -> Self::Output {
            Le32(self.0 + other.0)
        }
    }

    impl Sub for Le32 {
        type Output = Le32;

        fn sub(self, other: Self) -> Self::Output {
            Le32(self.0 - other.0)
        }
    }

    impl Mul for Le32 {
        type Output = Le32;

        fn mul(self, rhs: Self) -> Self::Output {
            Le32(self.0 * rhs.0)
        }
    }

    impl BitAnd for Le32 {
        type Output = Le32;
        
        fn bitand(self, rhs: Le32) -> Self::Output {
            Le32(self.0 & rhs.0)
        }
    }

    impl BitAndAssign for Le32 {
        fn bitand_assign(&mut self, rhs: Le32) {
            *self = Le32(self.0 & rhs.0)
        }
    }

    impl BitOr for Le32 {
        type Output = Le32;

        fn bitor(self, rhs: Le32) -> Self::Output {
            Le32(self.0 | rhs.0)
        }
    }

    impl BitOrAssign for Le32 {
        fn bitor_assign(&mut self, rhs: Le32) {
            *self = Le32(self.0 | rhs.0) 
        }
    }

    impl BitXor for Le32 {
        type Output = Le32;

        fn bitxor(self, rhs: Le32) -> Self::Output {
            Le32(self.0 ^ rhs.0)
        }
    }

    impl BitXorAssign for Le32 {
        fn bitxor_assign(&mut self, rhs: Le32) {
           *self = Le32(self.0 ^ rhs.0) 
        }
    }

    /// Native endian u64 indicating this value represents a little endian u64.
    /// The value is not stored as a little endian u64 inside the struct in order to 
    /// allow correct math operations without transformating it all the time into the 
    /// native endianess of the machine.
    ///
    /// In order to receive a little endian coded u64 one must use the 
    /// [as_u64()](Le64::as_u64) function. Native endian coded u64 can be retrieved via the
    /// [to_ne64()](Le64::to_ne_u64) function.
    #[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
    pub struct Le64(u64);

    impl Le64 {
        /// Returns the wrapped u32 as a little endian coded u32.
        pub fn as_le(self) -> u64 {
            self.0.to_le()
        }

        /// Returns the wrapped u32, which is native endian coded!
        pub fn as_ne(self) -> u64 {
            self.0
        }
    }

    impl From<u64> for Le64 {
        fn from(val: u64) -> Le64 {
            Le64(val)
        }
    }

    impl Add for Le64 {
        type Output = Le64;

        fn add(self, other: Self ) -> Self::Output {
            Le64(self.0 + other.0)
        }
    }

    impl Sub for Le64 {
        type Output = Le64;

        fn sub(self, other: Self) -> Self::Output {
            Le64(self.0 - other.0)
        }
    }

    impl Mul for Le64 {
        type Output = Le64;

        fn mul(self, rhs: Self) -> Self::Output {
            Le64(self.0 * rhs.0)
        }
    }

    impl BitAnd for Le64 {
        type Output = Le64;
        
        fn bitand(self, rhs: Le64) -> Self::Output {
            Le64(self.0 & rhs.0)
        }
    }

    impl BitAndAssign for Le64 {
        fn bitand_assign(&mut self, rhs: Le64) {
            *self = Le64(self.0 & rhs.0)
        }
    }

    impl BitOr for Le64 {
        type Output = Le64;

        fn bitor(self, rhs: Le64) -> Self::Output {
            Le64(self.0 | rhs.0)
        }
    }

    impl BitOrAssign for Le64 {
        fn bitor_assign(&mut self, rhs: Le64) {
            *self = Le64(self.0 | rhs.0) 
        }
    }

    impl BitXor for Le64 {
        type Output = Le64;

        fn bitxor(self, rhs: Le64) -> Self::Output {
            Le64(self.0 ^ rhs.0)
        }
    }

    impl BitXorAssign for Le64 {
        fn bitxor_assign(&mut self, rhs: Le64) {
           *self = Le64(self.0 ^ rhs.0) 
        }
    }


    /// Testing the virtio types module.
    #[cfg(test)]
    pub mod test {
        use crate::drivers::virtio::types::{Le16, Le32, Le64, Be16, Be32, Be64};

        #[test]
        fn adding_le16() {
            let result = Le16::from(4) + Le16::from(2);
            assert_eq!(result.to_ne_u16(), 6);
        }

        #[test]
        fn adding_le32() {
            let result = Le32::from(4) + Le32::from(2);
            assert_eq!(result.to_ne_u32(), 6);
        }
        
        #[test]
        fn adding_le64() {
            let result = Le64::from(4) + Le64::from(2);
            assert_eq!(result.to_ne_u64(), 6);
        }

        #[test]
        fn adding_be16() {
            let result = Be16::from(4) + Be16::from(2);
            assert_eq!(result.to_ne_u16(), 6);
        }

        #[test]
        fn adding_be32() {
            let result = Be32::from(4) + Be32::from(2);
            assert_eq!(result.to_ne_u32(), 6);
        }
        
        #[test]
        fn adding_be64() {
            let result = Be64::from(4) + Be64::from(2);
            assert_eq!(result.to_ne_u64(), 6);
        }

        #[test]
        fn sub_le16() {
            let result = Le16::from(4) - Le16::from(2);
            assert_eq!(result.to_ne_u16(), 2);
        }

        #[test]
        fn sub_be16() {
            let result = Be16::from(4) - Be16::from(2);
            assert_eq!(result.to_ne_u16(), 2);
        }

        #[test]
        fn sub_le32() {
            let result = Le32::from(4) - Le32::from(2);
            assert_eq!(result.to_ne_u32(), 2);
        }

        #[test]
        fn sub_be32() {
            let result = Be32::from(4) - Be32::from(2);
            assert_eq!(result.to_ne_u32(), 2);
        }

        #[test]
        fn sub_le64() {
            let result = Le64::from(4) - Le64::from(2);
            assert_eq!(result.to_ne_u64(), 2);
        }

        #[test]
        fn sub_be64() {
            let result = Be64::from(4) - Be64::from(2);
            assert_eq!(result.to_ne_u64(), 2);
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