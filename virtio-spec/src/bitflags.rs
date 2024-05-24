macro_rules! _bitflags_base {
    (
        $(#[$outer:meta])*
        $vis:vis struct $BitFlags:ident: $T:ty;

        $($t:tt)*
    ) => {
        #[cfg_attr(
            feature = "zerocopy",
            derive(
                zerocopy_derive::FromZeroes,
                zerocopy_derive::FromBytes,
                zerocopy_derive::AsBytes
            )
        )]
        #[derive(Default, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(transparent)]
        $(#[$outer])*
        $vis struct $BitFlags($T);

        impl ::core::fmt::Debug for $BitFlags {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                struct Inner<'a>(&'a $BitFlags);

                impl<'a> ::core::fmt::Debug for Inner<'a> {
                    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                        if self.0.is_empty() {
                            f.write_str("0x0")
                        } else {
                            ::bitflags::parser::to_writer(self.0, f)
                        }
                    }
                }

                f.debug_tuple(::core::stringify!($BitFlags))
                    .field(&Inner(self))
                    .finish()
            }
        }

        _bitflags_base! {
            $($t)*
        }
    };
    () => {};
}

macro_rules! virtio_bitflags {
    (
        $(#[$outer:meta])*
        $vis:vis struct $BitFlags:ident: $T:ty {
            $(
                $(#[$inner:ident $($args:tt)*])*
                const $Flag:tt = $value:expr;
            )*
        }

        $($t:tt)*
    ) => {
        _bitflags_base! {
            $(#[$outer])*
            $vis struct $BitFlags: $T;
        }

        ::bitflags::bitflags! {
            impl $BitFlags: $T {
                $(
                    $(#[$inner $($args)*])*
                    const $Flag = $value;
                )*

                const _ = !0;
            }
        }

        virtio_bitflags! {
            $($t)*
        }
    };
    () => {};
}

macro_rules! impl_fmt {
    ($Trait:ident for $SelfT:ty) => {
        impl ::core::fmt::$Trait for $SelfT {
            fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

macro_rules! endian_bitflags {
    (
        $(#[$outer:meta])*
        $vis:vis struct $BitFlags:ident: $T:ty {
            $(
                $(#[$inner:ident $($args:tt)*])*
                const $Flag:tt = $value:expr;
            )*
        }

        $($t:tt)*
    ) => {
        _bitflags_base! {
            $(#[$outer])*
            $vis struct $BitFlags: $T;
        }

        impl $BitFlags {
            $(
                $(#[$inner $($args)*])*
                pub const $Flag: Self = Self(<$T>::from_ne($value));
            )*
        }

        impl ::bitflags::Flags for $BitFlags {
            const FLAGS: &'static [::bitflags::Flag<Self>] = &[
                $(
                    ::bitflags::Flag::new(::core::stringify!($Flag), Self::$Flag),
                )*
                ::bitflags::Flag::new("", Self::all()),
            ];

            type Bits = $T;

            fn from_bits_retain(bits: Self::Bits) -> Self {
                Self(bits)
            }

            fn bits(&self) -> Self::Bits {
                self.0
            }
        }

        impl $BitFlags{
            /// Get a flags value with all bits unset.
            #[inline]
            pub const fn empty() -> Self {
                Self(<$T as ::bitflags::Bits>::EMPTY)
            }

            /// Get a flags value with all known bits set.
            #[inline]
            pub const fn all() -> Self {
                Self(<$T as ::bitflags::Bits>::ALL)
            }

            /// Get the underlying bits value.
            ///
            /// The returned value is exactly the bits set in this flags value.
            #[inline]
            pub const fn bits(&self) -> $T {
                self.0
            }

            /// Convert from a bits value.
            ///
            /// This method will return `None` if any unknown bits are set.
            #[inline]
            pub const fn from_bits(bits: $T) -> Option<Self> {
                Some(Self(bits))
            }

            /// Convert from a bits value, unsetting any unknown bits.
            #[inline]
            pub const fn from_bits_truncate(bits: $T) -> Self {
                Self(bits)
            }

            /// Convert from a bits value exactly.
            #[inline]
            pub const fn from_bits_retain(bits: $T) -> Self {
                Self(bits)
            }

            /// Get a flags value with the bits of a flag with the given name set.
            ///
            /// This method will return `None` if `name` is empty or doesn't
            /// correspond to any named flag.
            #[inline]
            pub fn from_name(name: &str) -> Option<Self> {
                <Self as ::bitflags::Flags>::from_name(name)
            }

            /// Whether all bits in this flags value are unset.
            #[inline]
            pub const fn is_empty(&self) -> bool {
                self.bits().to_ne() == <$T as ::bitflags::Bits>::EMPTY.to_ne()
            }

            /// Whether all known bits in this flags value are set.
            #[inline]
            pub const fn is_all(&self) -> bool {
                Self::all().bits().to_ne() | self.bits().to_ne() == self.bits().to_ne()
            }

            /// Whether any set bits in a source flags value are also set in a target flags value.
            #[inline]
            pub const fn intersects(&self, other: Self) -> bool {
                self.bits().to_ne() & other.bits().to_ne() != <$T as ::bitflags::Bits>::EMPTY.to_ne()
            }

            /// Whether all set bits in a source flags value are also set in a target flags value.
            #[inline]
            pub const fn contains(&self, other: Self) -> bool {
                self.bits().to_ne() & other.bits().to_ne() == other.bits().to_ne()
            }

            /// The bitwise or (`|`) of the bits in two flags values.
            #[inline]
            pub fn insert(&mut self, other: Self) {
                *self = Self::from_bits_retain(self.bits()).union(other);
            }

            /// The intersection of a source flags value with the complement of a target flags value (`&!`).
            ///
            /// This method is not equivalent to `self & !other` when `other` has unknown bits set.
            /// `remove` won't truncate `other`, but the `!` operator will.
            #[inline]
            pub fn remove(&mut self, other: Self) {
                *self = Self::from_bits_retain(self.bits()).difference(other);
            }

            /// The bitwise exclusive-or (`^`) of the bits in two flags values.
            #[inline]
            pub fn toggle(&mut self, other: Self) {
                *self = Self::from_bits_retain(self.bits()).symmetric_difference(other);
            }

            /// Call `insert` when `value` is `true` or `remove` when `value` is `false`.
            #[inline]
            pub fn set(&mut self, other: Self, value: bool) {
                if value {
                    self.insert(other);
                } else {
                    self.remove(other);
                }
            }

            /// The bitwise and (`&`) of the bits in two flags values.
            #[inline]
            #[must_use]
            pub const fn intersection(self, other: Self) -> Self {
                Self::from_bits_retain(<$T>::from_ne(self.bits().to_ne() & other.bits().to_ne()))
            }

            /// The bitwise or (`|`) of the bits in two flags values.
            #[inline]
            #[must_use]
            pub const fn union(self, other: Self) -> Self {
                Self::from_bits_retain(<$T>::from_ne(self.bits().to_ne() | other.bits().to_ne()))
            }

            /// The intersection of a source flags value with the complement of a target flags value (`&!`).
            ///
            /// This method is not equivalent to `self & !other` when `other` has unknown bits set.
            /// `difference` won't truncate `other`, but the `!` operator will.
            #[inline]
            #[must_use]
            pub const fn difference(self, other: Self) -> Self {
                Self::from_bits_retain(<$T>::from_ne(self.bits().to_ne() & !other.bits().to_ne()))
            }

            /// The bitwise exclusive-or (`^`) of the bits in two flags values.
            #[inline]
            #[must_use]
            pub const fn symmetric_difference(self, other: Self) -> Self {
                Self::from_bits_retain(<$T>::from_ne(self.bits().to_ne() ^ other.bits().to_ne()))
            }

            /// The bitwise negation (`!`) of the bits in a flags value, truncating the result.
            #[inline]
            #[must_use]
            pub const fn complement(self) -> Self {
                Self::from_bits_truncate(<$T>::from_ne(!self.bits().to_ne()))
            }
        }

        impl_fmt!(Binary for $BitFlags);
        impl_fmt!(Octal for $BitFlags);
        impl_fmt!(LowerHex for $BitFlags);
        impl_fmt!(UpperHex for $BitFlags);

        impl ::core::ops::BitOr for $BitFlags {
            type Output = Self;

            /// The bitwise or (`|`) of the bits in two flags values.
            #[inline]
            fn bitor(self, other: Self) -> Self {
                self.union(other)
            }
        }

        impl ::core::ops::BitOrAssign for $BitFlags {
            /// The bitwise or (`|`) of the bits in two flags values.
            #[inline]
            fn bitor_assign(&mut self, other: Self) {
                self.insert(other);
            }
        }

        impl ::core::ops::BitXor for $BitFlags {
            type Output = Self;

            /// The bitwise exclusive-or (`^`) of the bits in two flags values.
            #[inline]
            fn bitxor(self, other: Self) -> Self {
                self.symmetric_difference(other)
            }
        }

        impl ::core::ops::BitXorAssign for $BitFlags {
            /// The bitwise exclusive-or (`^`) of the bits in two flags values.
            #[inline]
            fn bitxor_assign(&mut self, other: Self) {
                self.toggle(other);
            }
        }

        impl ::core::ops::BitAnd for $BitFlags {
            type Output = Self;

            /// The bitwise and (`&`) of the bits in two flags values.
            #[inline]
            fn bitand(self, other: Self) -> Self {
                self.intersection(other)
            }
        }

        impl ::core::ops::BitAndAssign for $BitFlags {
            /// The bitwise and (`&`) of the bits in two flags values.
            #[inline]
            fn bitand_assign(&mut self, other: Self) {
                *self = Self::from_bits_retain(self.bits()).intersection(other);
            }
        }

        impl ::core::ops::Sub for $BitFlags {
            type Output = Self;

            /// The intersection of a source flags value with the complement of a target flags value (`&!`).
            ///
            /// This method is not equivalent to `self & !other` when `other` has unknown bits set.
            /// `difference` won't truncate `other`, but the `!` operator will.
            #[inline]
            fn sub(self, other: Self) -> Self {
                self.difference(other)
            }
        }

        impl ::core::ops::SubAssign for $BitFlags {
            /// The intersection of a source flags value with the complement of a target flags value (`&!`).
            ///
            /// This method is not equivalent to `self & !other` when `other` has unknown bits set.
            /// `difference` won't truncate `other`, but the `!` operator will.
            #[inline]
            fn sub_assign(&mut self, other: Self) {
                self.remove(other);
            }
        }

        impl ::core::ops::Not for $BitFlags {
            type Output = Self;

            /// The bitwise negation (`!`) of the bits in a flags value, truncating the result.
            #[inline]
            fn not(self) -> Self {
                self.complement()
            }
        }

        impl ::core::iter::Extend<$BitFlags> for $BitFlags {
            /// The bitwise or (`|`) of the bits in each flags value.
            fn extend<T>(&mut self, iterator: T)
            where
                T: ::core::iter::IntoIterator<Item = Self>,
            {
                for item in iterator {
                    self.insert(item)
                }
            }
        }

        impl ::core::iter::FromIterator<$BitFlags> for $BitFlags {
            /// The bitwise or (`|`) of the bits in each flags value.
            fn from_iter<T>(iterator: T) -> Self
            where
                T: ::core::iter::IntoIterator<Item = Self>,
            {
                use ::core::iter::Extend;

                let mut result = Self::empty();
                result.extend(iterator);
                result
            }
        }

        impl $BitFlags {
            /// Yield a set of contained flags values.
            ///
            /// Each yielded flags value will correspond to a defined named flag. Any unknown bits
            /// will be yielded together as a final flags value.
            #[inline]
            pub fn iter(&self) -> ::bitflags::iter::Iter<Self> {
                ::bitflags::Flags::iter(self)
            }

            /// Yield a set of contained named flags values.
            ///
            /// This method is like [`iter`](#method.iter), except only yields bits in contained named flags.
            /// Any unknown bits, or bits not corresponding to a contained flag will not be yielded.
            #[inline]
            pub fn iter_names(&self) -> ::bitflags::iter::IterNames<Self> {
                ::bitflags::Flags::iter_names(self)
            }
        }

        impl ::core::iter::IntoIterator for $BitFlags {
            type Item = Self;
            type IntoIter = ::bitflags::iter::Iter<Self::Item>;
            fn into_iter(self) -> Self::IntoIter {
                self.iter()
            }
        }

        endian_bitflags! {
            $($t)*
        }
    };
    () => {};
}
