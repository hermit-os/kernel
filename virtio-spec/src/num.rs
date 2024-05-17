//! Byte order-aware numeric primitives.

use core::ops::{BitAnd, BitOr, BitXor, Not};
use core::{fmt, mem};

use bitflags::parser::{ParseError, ParseHex, WriteHex};
use bitflags::Bits;

macro_rules! le_impl {
    ($SelfT:ident, $ActualT:ty, $to:ident, $from:ident, $bits:expr, $order:expr) => {
        #[doc = concat!("A ", stringify!($bits), "-bit unsigned integer stored in ", $order, " byte order.")]
        #[allow(non_camel_case_types)]
        #[must_use]
        #[cfg_attr(feature = "zerocopy", derive(zerocopy_derive::FromZeroes, zerocopy_derive::FromBytes, zerocopy_derive::AsBytes))]
        #[derive(Default, Hash, PartialEq, Eq, Clone, Copy, Debug)]
        #[repr(transparent)]
        pub struct $SelfT($ActualT);

        impl $SelfT {
            #[doc = concat!("Creates a new ", $order, "integer from native-endian byte order.")]
            #[inline]
            pub const fn new(n: $ActualT) -> Self {
                Self(n.$to())
            }

            /// Returns the integer in native-endian byte order.
            #[inline]
            pub const fn get(self) -> $ActualT {
                <$ActualT>::$from(self.0)
            }
        }

        impl From<$ActualT> for $SelfT {
            #[inline]
            fn from(value: $ActualT) -> Self {
                Self::new(value)
            }
        }

        impl From<$SelfT> for $ActualT {
            #[inline]
            fn from(value: $SelfT) -> Self {
                value.get()
            }
        }

        impl Bits for $SelfT {
            const EMPTY: Self = Self::new(0);

            const ALL: Self = Self::new(<$ActualT>::MAX);
        }

        impl ParseHex for $SelfT {
            fn parse_hex(input: &str) -> Result<Self, ParseError> {
                <$ActualT>::parse_hex(input).map(Self::from)
            }
        }

        impl WriteHex for $SelfT {
            fn write_hex<W: fmt::Write>(&self, writer: W) -> fmt::Result {
                self.get().write_hex(writer)
            }
        }

        impl BitAnd for $SelfT {
            type Output = Self;

            fn bitand(self, rhs: Self) -> Self::Output {
                Self::new(self.get().bitand(rhs.get()))
            }
        }

        impl BitOr for $SelfT {
            type Output = Self;

            fn bitor(self, rhs: Self) -> Self::Output {
                Self::new(self.get().bitor(rhs.get()))
            }
        }

        impl BitXor for $SelfT {
            type Output = Self;

            fn bitxor(self, rhs: Self) -> Self::Output {
                Self::new(self.get().bitxor(rhs.get()))
            }
        }

        impl Not for $SelfT {
            type Output = Self;

            fn not(self) -> Self::Output {
                Self::new(self.get().not())
            }
        }
    };
}

le_impl!(be16, u16, to_be, from_be, 16, "big-endian");
le_impl!(be32, u32, to_be, from_be, 32, "big-endian");
le_impl!(be64, u64, to_be, from_be, 64, "big-endian");
le_impl!(be128, u128, to_be, from_be, 128, "big-endian");
le_impl!(le16, u16, to_le, from_le, 16, "little-endian");
le_impl!(le32, u32, to_le, from_le, 32, "little-endian");
le_impl!(le64, u64, to_le, from_le, 64, "little-endian");
le_impl!(le128, u128, to_le, from_le, 128, "little-endian");

impl be64 {
    /// Create an integer from its representation as a [`be32`] array in big endian.
    pub const fn from_be_parts(parts: [be32; 2]) -> Self {
        unsafe { mem::transmute(parts) }
    }

    /// Return the memory representation of this integer as a [`be32`] array in big-endian (network) byte order.
    pub const fn to_be_parts(self) -> [be32; 2] {
        unsafe { mem::transmute(self) }
    }
}

impl le64 {
    /// Create an integer from its representation as a [`le32`] array in little endian.
    pub const fn from_le_parts(parts: [le32; 2]) -> Self {
        unsafe { mem::transmute(parts) }
    }

    /// Return the memory representation of this integer as a [`le32`] array in little-endian byte order.
    pub const fn to_le_parts(self) -> [le32; 2] {
        unsafe { mem::transmute(self) }
    }
}

impl be128 {
    /// Create an integer from its representation as a [`be32`] array in big endian.
    pub const fn from_be_parts(parts: [be32; 4]) -> Self {
        unsafe { mem::transmute(parts) }
    }

    /// Return the memory representation of this integer as a [`be32`] array in big-endian (network) byte order.
    pub const fn to_be_parts(self) -> [be32; 4] {
        unsafe { mem::transmute(self) }
    }
}

impl le128 {
    /// Create an integer from its representation as a [`le32`] array in little endian.
    pub const fn from_le_parts(parts: [le32; 4]) -> Self {
        unsafe { mem::transmute(parts) }
    }

    /// Return the memory representation of this integer as a [`le32`] array in little-endian byte order.
    pub const fn to_le_parts(self) -> [le32; 4] {
        unsafe { mem::transmute(self) }
    }
}
