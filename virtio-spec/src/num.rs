//! Byte order-aware numeric primitives.

use core::ops::{BitAnd, BitOr, BitXor, Not};
use core::{fmt, mem};

use bitflags::parser::{ParseError, ParseHex, WriteHex};
use bitflags::Bits;

/// An unsigned integer stored in big-endian byte order.
#[cfg_attr(
    feature = "zerocopy",
    derive(
        zerocopy_derive::FromZeroes,
        zerocopy_derive::FromBytes,
        zerocopy_derive::AsBytes
    )
)]
#[derive(Default, Hash, PartialEq, Eq, Clone, Copy, Debug)]
#[repr(transparent)]
pub struct Be<T>(T);

/// An unsigned integer stored in little-endian byte order.
#[cfg_attr(
    feature = "zerocopy",
    derive(
        zerocopy_derive::FromZeroes,
        zerocopy_derive::FromBytes,
        zerocopy_derive::AsBytes
    )
)]
#[derive(Default, Hash, PartialEq, Eq, Clone, Copy, Debug)]
#[repr(transparent)]
pub struct Le<T>(T);

macro_rules! endian_impl {
    ($SelfT:ident, $ActualT:ty, $alias:ident, $to:ident, $from:ident, $bits:expr, $order:expr) => {
        #[doc = concat!("A ", stringify!($bits), "-bit unsigned integer stored in ", $order, " byte order.")]
        #[allow(non_camel_case_types)]
        pub type $alias = $SelfT<$ActualT>;

        impl $SelfT<$ActualT> {
            #[doc = concat!("Creates a new ", $order, " integer from native-endian byte order.")]
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

        impl From<$ActualT> for $SelfT<$ActualT> {
            #[inline]
            fn from(value: $ActualT) -> Self {
                Self::new(value)
            }
        }

        impl From<$SelfT<$ActualT>> for $ActualT {
            #[inline]
            fn from(value: $SelfT<$ActualT>) -> Self {
                value.get()
            }
        }

        impl Bits for $SelfT<$ActualT> {
            const EMPTY: Self = Self::new(0);

            const ALL: Self = Self::new(<$ActualT>::MAX);
        }

        impl ParseHex for $SelfT<$ActualT> {
            fn parse_hex(input: &str) -> Result<Self, ParseError> {
                <$ActualT>::parse_hex(input).map(Self::from)
            }
        }

        impl WriteHex for $SelfT<$ActualT> {
            fn write_hex<W: fmt::Write>(&self, writer: W) -> fmt::Result {
                self.get().write_hex(writer)
            }
        }

        impl BitAnd for $SelfT<$ActualT> {
            type Output = Self;

            fn bitand(self, rhs: Self) -> Self::Output {
                Self::new(self.get().bitand(rhs.get()))
            }
        }

        impl BitOr for $SelfT<$ActualT> {
            type Output = Self;

            fn bitor(self, rhs: Self) -> Self::Output {
                Self::new(self.get().bitor(rhs.get()))
            }
        }

        impl BitXor for $SelfT<$ActualT> {
            type Output = Self;

            fn bitxor(self, rhs: Self) -> Self::Output {
                Self::new(self.get().bitxor(rhs.get()))
            }
        }

        impl Not for $SelfT<$ActualT> {
            type Output = Self;

            fn not(self) -> Self::Output {
                Self::new(self.get().not())
            }
        }
    };
}

endian_impl!(Be, u16, be16, to_be, from_be, 16, "big-endian");
endian_impl!(Be, u32, be32, to_be, from_be, 32, "big-endian");
endian_impl!(Be, u64, be64, to_be, from_be, 64, "big-endian");
endian_impl!(Be, u128, be128, to_be, from_be, 128, "big-endian");
endian_impl!(Le, u16, le16, to_le, from_le, 16, "little-endian");
endian_impl!(Le, u32, le32, to_le, from_le, 32, "little-endian");
endian_impl!(Le, u64, le64, to_le, from_le, 64, "little-endian");
endian_impl!(Le, u128, le128, to_le, from_le, 128, "little-endian");

impl Be<u64> {
    /// Create an integer from its representation as a [`Be<u32>`] array in big endian.
    pub const fn from_be_parts(parts: [Be<u32>; 2]) -> Self {
        unsafe { mem::transmute(parts) }
    }

    /// Return the memory representation of this integer as a [`Be<u32>`] array in big-endian (network) byte order.
    pub const fn to_be_parts(self) -> [Be<u32>; 2] {
        unsafe { mem::transmute(self) }
    }
}

impl Le<u64> {
    /// Create an integer from its representation as a [`Le<u32>`] array in little endian.
    pub const fn from_le_parts(parts: [Le<u32>; 2]) -> Self {
        unsafe { mem::transmute(parts) }
    }

    /// Return the memory representation of this integer as a [`Le<u32>`] array in little-endian byte order.
    pub const fn to_le_parts(self) -> [Le<u32>; 2] {
        unsafe { mem::transmute(self) }
    }
}

impl Be<u128> {
    /// Create an integer from its representation as a [`Be<u32>`] array in big endian.
    pub const fn from_be_parts(parts: [Be<u32>; 4]) -> Self {
        unsafe { mem::transmute(parts) }
    }

    /// Return the memory representation of this integer as a [`Be<u32>`] array in big-endian (network) byte order.
    pub const fn to_be_parts(self) -> [Be<u32>; 4] {
        unsafe { mem::transmute(self) }
    }
}

impl Le<u128> {
    /// Create an integer from its representation as a [`Le<u32>`] array in little endian.
    pub const fn from_le_parts(parts: [Le<u32>; 4]) -> Self {
        unsafe { mem::transmute(parts) }
    }

    /// Return the memory representation of this integer as a [`Le<u32>`] array in little-endian byte order.
    pub const fn to_le_parts(self) -> [Le<u32>; 4] {
        unsafe { mem::transmute(self) }
    }
}
