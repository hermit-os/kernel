//! This crate provides common definitions from the Virtio 1.2 specification.
//! This crate does not provide any additional driver-related functionality.
//!
//! For the actual specification see [Virtual I/O Device (VIRTIO) Version 1.2—Committee Specification 01](https://docs.oasis-open.org/virtio/virtio/v1.2/cs01/virtio-v1.2-cs01.html).

#![no_std]

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

        ::bitflags::bitflags! {
            impl $BitFlags: $T {
                $(
                    $(#[$inner $($args)*])*
                    const $Flag = $value;
                )*
            }
        }

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

        virtio_bitflags! {
            $($t)*
        }
    };
    () => {};
}

mod features;
pub mod net;
pub mod num;
pub mod pci;

pub use features::F;

pub mod fs {
    //! File System Device

    pub use super::features::fs::F;
}

virtio_bitflags! {
    /// Device Status Field
    ///
    /// During device initialization by a driver,
    /// the driver follows the sequence of steps specified in
    /// _General Initialization And Device Operation / Device
    /// Initialization_.
    ///
    /// The `device status` field provides a simple low-level
    /// indication of the completed steps of this sequence.
    /// It's most useful to imagine it hooked up to traffic
    /// lights on the console indicating the status of each device.  The
    /// following bits are defined (listed below in the order in which
    /// they would be typically set):
    pub struct DeviceStatus: u8 {
        /// Indicates that the guest OS has found the
        /// device and recognized it as a valid virtio device.
        const ACKNOWLEDGE = 1;

        /// Indicates that the guest OS knows how to drive the
        /// device.
        ///
        /// <div class="warning">
        ///
        /// There could be a significant (or infinite) delay before setting
        /// this bit.  For example, under Linux, drivers can be loadable modules.
        ///
        /// </div>
        const DRIVER = 2;

        /// Indicates that something went wrong in the guest,
        /// and it has given up on the device. This could be an internal
        /// error, or the driver didn't like the device for some reason, or
        /// even a fatal error during device operation.
        const FAILED = 128;

        /// Indicates that the driver has acknowledged all the
        /// features it understands, and feature negotiation is complete.
        const FEATURES_OK = 8;

        /// Indicates that the driver is set up and ready to
        /// drive the device.
        const DRIVER_OK = 4;

        /// Indicates that the device has experienced
        /// an error from which it can't recover.
        const DEVICE_NEEDS_RESET = 64;
    }
}
