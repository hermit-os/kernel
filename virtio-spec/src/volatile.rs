//! Volatile Pointer Types.

use core::marker::PhantomData;

use volatile::access::{Readable, Writable};
use volatile::VolatilePtr;

use crate::mmio::InterruptStatus;
use crate::{be32, be64, le16, le32, le64, DeviceStatus, Id};

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

    /// Updates the contained value using the given closure and volatile instructions.
    ///
    /// See [`VolatilePtr::update`].
    pub fn update(self, f: impl FnOnce(le64) -> le64)
    where
        A: Readable + Writable,
    {
        let new = f(self.read());
        self.write(new);
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

    /// Updates the contained value using the given closure and volatile instructions.
    ///
    /// See [`VolatilePtr::update`].
    pub fn update(self, f: impl FnOnce(be64) -> be64)
    where
        A: Readable + Writable,
    {
        let new = f(self.read());
        self.write(new);
    }
}

macro_rules! impl_wide_field_access {
    (
        $(#[$outer:meta])*
        $vis:vis trait $Trait:ident<'a, A>: $T:ty {
            $(
                $(#[doc = $doc:literal])*
                $(#[doc(alias = $alias:literal)])?
                #[access($Access:ty)]
                $field:ident: $field_low:ident, $field_high:ident;
            )*
        }
    ) => {
        $(#[$outer])*
        $vis trait $Trait<'a, A> {
            $(
                $(#[doc = $doc])*
                $(#[doc(alias = $alias)])?
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

/// An overaligned volatile pointer for fields that require wider access operations.
///
/// In virtio, some fields require wider access operations than their type indicate, such as for [`mmio::DeviceRegisters`].
///
/// [`mmio::DeviceRegisters`]: crate::mmio::DeviceRegisters
pub struct OveralignedVolatilePtr<'a, T, F, A>
where
    T: ?Sized,
    F: ?Sized,
{
    ptr: VolatilePtr<'a, F, A>,
    ty: PhantomData<VolatilePtr<'a, T, A>>,
}

impl<'a, T, F, A> Copy for OveralignedVolatilePtr<'a, T, F, A>
where
    T: ?Sized,
    F: ?Sized,
{
}

impl<'a, T, F, A> Clone for OveralignedVolatilePtr<'a, T, F, A>
where
    T: ?Sized,
    F: ?Sized,
{
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a, T, F, A> OveralignedVolatilePtr<'a, T, F, A>
where
    T: OveralignedField<F>,
    F: Copy,
{
    /// Creates a new overaligned volatile pointer.
    pub fn new(ptr: VolatilePtr<'a, F, A>) -> Self {
        Self {
            ptr,
            ty: PhantomData,
        }
    }

    /// Performs a volatile read of the contained value.
    ///
    /// See [`VolatilePtr::read`].
    pub fn read(self) -> T
    where
        A: Readable,
    {
        T::from_field(self.ptr.read())
    }

    /// Performs a volatile write, setting the contained value to the given `value`.
    ///
    /// See [`VolatilePtr::write`].
    pub fn write(self, value: T)
    where
        A: Writable,
    {
        self.ptr.write(value.into_field())
    }

    /// Updates the contained value using the given closure and volatile instructions.
    ///
    /// See [`VolatilePtr::update`].
    pub fn update(self, f: impl FnOnce(T) -> T)
    where
        A: Readable + Writable,
    {
        let new = f(self.read());
        self.write(new);
    }
}

/// A trait for fields that can be accessed via [`OveralignedVolatilePtr`].
pub trait OveralignedField<F>: private::Sealed<F> {
    /// Converts to this type from the overaligned field.
    fn from_field(field: F) -> Self;

    /// Converts this type into the overaligned field.
    fn into_field(self) -> F;
}

impl OveralignedField<le32> for le16 {
    fn from_field(field: le32) -> Self {
        field.try_into().unwrap()
    }

    fn into_field(self) -> le32 {
        self.into()
    }
}

impl OveralignedField<le32> for bool {
    fn from_field(field: le32) -> Self {
        field.to_ne() == 1
    }

    fn into_field(self) -> le32 {
        le32::from_ne(self as u32)
    }
}

impl OveralignedField<le32> for u8 {
    fn from_field(field: le32) -> Self {
        field.to_ne().try_into().unwrap()
    }

    fn into_field(self) -> le32 {
        le32::from_ne(self.into())
    }
}

impl OveralignedField<le32> for Id {
    fn from_field(field: le32) -> Self {
        Self::from(u8::from_field(field))
    }

    fn into_field(self) -> le32 {
        u8::from(self).into_field()
    }
}

impl OveralignedField<le32> for DeviceStatus {
    fn from_field(field: le32) -> Self {
        Self::from_bits_retain(u8::from_field(field))
    }

    fn into_field(self) -> le32 {
        self.bits().into_field()
    }
}

impl OveralignedField<le32> for InterruptStatus {
    fn from_field(field: le32) -> Self {
        Self::from_bits_retain(u8::from_field(field))
    }

    fn into_field(self) -> le32 {
        self.bits().into_field()
    }
}

mod private {
    use crate::mmio::InterruptStatus;
    use crate::{le16, le32, DeviceStatus, Id};

    pub trait Sealed<T> {}

    impl Sealed<le32> for bool {}
    impl Sealed<le32> for u8 {}
    impl Sealed<le32> for le16 {}
    impl Sealed<le32> for Id {}
    impl Sealed<le32> for DeviceStatus {}
    impl Sealed<le32> for InterruptStatus {}
}
