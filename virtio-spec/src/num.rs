//! Byte order-aware numeric primitives.

use core::cmp::Ordering;
use core::{fmt, mem, ops};

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
#[derive(Default, Hash, PartialEq, Eq, Clone, Copy)]
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
#[derive(Default, Hash, PartialEq, Eq, Clone, Copy)]
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

macro_rules! impl_fmt {
    ($Trait:ident, $SelfT:ident) => {
        impl<T> fmt::$Trait for $SelfT<T>
        where
            Self: Copy + Into<T>,
            T: fmt::$Trait,
        {
            #[inline]
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                (*self).into().fmt(f)
            }
        }
    };
}

macro_rules! impl_binary_op {
    ($Trait:ident, $op:ident, $TraitAssign:ident, $op_assign:ident, $SelfT:ident) => {
        impl<T> ops::$Trait for $SelfT<T>
        where
            Self: Into<T>,
            T: ops::$Trait<Output = T> + Into<Self>,
        {
            type Output = Self;

            #[inline]
            fn $op(self, rhs: Self) -> Self::Output {
                self.into().$op(rhs.into()).into()
            }
        }

        impl<T> ops::$TraitAssign for $SelfT<T>
        where
            Self: Copy + ops::$Trait<Output = Self>,
        {
            #[inline]
            fn $op_assign(&mut self, rhs: Self) {
                use ops::$Trait;

                *self = self.$op(rhs)
            }
        }
    };
}

macro_rules! impl_traits {
    ($SelfT:ident) => {
        impl_fmt!(Debug, $SelfT);
        impl_fmt!(Display, $SelfT);
        impl_fmt!(Binary, $SelfT);
        impl_fmt!(Octal, $SelfT);
        impl_fmt!(LowerHex, $SelfT);
        impl_fmt!(UpperHex, $SelfT);

        impl_binary_op!(Add, add, AddAssign, add_assign, $SelfT);
        impl_binary_op!(BitAnd, bitand, BitAndAssign, bitand_assign, $SelfT);
        impl_binary_op!(BitOr, bitor, BitOrAssign, bitor_assign, $SelfT);
        impl_binary_op!(BitXor, bitxor, BitXorAssign, bitxor_assign, $SelfT);
        impl_binary_op!(Div, div, DivAssign, div_assign, $SelfT);
        impl_binary_op!(Mul, mul, MulAssign, mul_assign, $SelfT);
        impl_binary_op!(Rem, rem, RemAssign, rem_assign, $SelfT);
        impl_binary_op!(Shl, shl, ShlAssign, shl_assign, $SelfT);
        impl_binary_op!(Shr, shr, ShrAssign, shr_assign, $SelfT);
        impl_binary_op!(Sub, sub, SubAssign, sub_assign, $SelfT);

        impl<T> ParseHex for $SelfT<T>
        where
            T: ParseHex + Into<Self>,
        {
            fn parse_hex(input: &str) -> Result<Self, ParseError> {
                T::parse_hex(input).map(Into::into)
            }
        }

        impl<T> WriteHex for $SelfT<T>
        where
            Self: Copy + Into<T>,
            T: WriteHex,
        {
            fn write_hex<W: fmt::Write>(&self, writer: W) -> fmt::Result {
                (*self).into().write_hex(writer)
            }
        }

        impl<T> ops::Not for $SelfT<T>
        where
            Self: Into<T>,
            T: ops::Not<Output = T> + Into<Self>,
        {
            type Output = Self;

            fn not(self) -> Self::Output {
                self.into().not().into()
            }
        }

        impl<T> PartialOrd for $SelfT<T>
        where
            Self: Copy + Into<T>,
            T: Ord,
        {
            #[inline]
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.cmp(other))
            }
        }

        impl<T> Ord for $SelfT<T>
        where
            Self: Copy + Into<T>,
            T: Ord,
        {
            #[inline]
            fn cmp(&self, other: &Self) -> Ordering {
                (*self).into().cmp(&(*other).into())
            }
        }
    };
}

impl_traits!(Be);
impl_traits!(Le);

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
