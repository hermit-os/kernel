//! Volatile Pointer Types.

use volatile::access::{Readable, Writable};
use volatile::VolatilePtr;

use crate::{be32, be64, le32, le64};

/// A wide volatile pointer for 64-bit fields.
///
/// In virtio, 64-bit fields are to be treated as two 32-bit fields, with low 32 bit part followed by the high 32 bit part.
/// This type mimics [`VolatilePtr`], and allows easy access to 64-bit fields.
pub struct WideVolatilePtr<'a, T, A>
where
    T: ?Sized,
{
    low: VolatilePtr<'a, T, A>,
    high: VolatilePtr<'a, T, A>,
}

impl<'a, T, A> Copy for WideVolatilePtr<'a, T, A> where T: ?Sized {}

impl<'a, T, A> Clone for WideVolatilePtr<'a, T, A>
where
    T: ?Sized,
{
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a, T, A> WideVolatilePtr<'a, T, A> {
    /// Creates a new wide volatile pointer from pointers to the low and to the high part.
    pub fn from_low_high(low: VolatilePtr<'a, T, A>, high: VolatilePtr<'a, T, A>) -> Self {
        Self { low, high }
    }
}

impl<'a, A> WideVolatilePtr<'a, le32, A> {
    /// Performs a volatile read of the contained value.
    ///
    /// See [`VolatilePtr::read`].
    pub fn read(self) -> le64
    where
        A: Readable,
    {
        let low = self.low.read();
        let high = self.high.read();
        le64::from([low, high])
    }

    /// Performs a volatile write, setting the contained value to the given `value`.
    ///
    /// See [`VolatilePtr::write`].
    pub fn write(self, value: le64)
    where
        A: Writable,
    {
        let [low, high] = value.into();
        self.low.write(low);
        self.high.write(high);
    }
}

impl<'a, A> WideVolatilePtr<'a, be32, A> {
    /// Performs a volatile read of the contained value.
    ///
    /// See [`VolatilePtr::read`].
    pub fn read(self) -> be64
    where
        A: Readable,
    {
        let low = self.low.read();
        let high = self.high.read();
        be64::from([low, high])
    }

    /// Performs a volatile write, setting the contained value to the given `value`.
    ///
    /// See [`VolatilePtr::write`].
    pub fn write(self, value: be64)
    where
        A: Writable,
    {
        let [low, high] = value.into();
        self.low.write(low);
        self.high.write(high);
    }
}

macro_rules! impl_wide_field_access {
    (
        $(#[$outer:meta])*
        $vis:vis trait $Trait:ident<'a, A>: $T:ty {
            $(
                $(#[doc = $doc:literal])*
                #[access($Access:ty)]
                $field:ident: $field_low:ident, $field_high:ident;
            )*
        }
    ) => {
        $(#[$outer])*
        $vis trait $Trait<'a, A> {
            $(
                $(#[doc = $doc])*
                fn $field(self) -> WideVolatilePtr<'a, le32, A::Restricted>
                where
                    A: RestrictAccess<$Access>;
            )*
        }

        impl<'a, A> $Trait<'a, A> for VolatilePtr<'a, $T, A> {
            $(
                fn $field(self) -> WideVolatilePtr<'a, le32, A::Restricted>
                where
                    A: RestrictAccess<$Access>,
                {
                    WideVolatilePtr::from_low_high(self.$field_low(), self.$field_high())
                }
            )*
        }
    };
}
