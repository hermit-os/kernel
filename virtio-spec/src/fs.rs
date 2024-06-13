//! File System Device

use volatile::access::ReadOnly;
use volatile_macro::VolatileFieldAccess;

pub use super::features::fs::F;
use super::le32;

/// Device configuration
#[doc(alias = "virtio_fs_config")]
#[cfg_attr(
    feature = "zerocopy",
    derive(zerocopy_derive::FromZeroes, zerocopy_derive::FromBytes)
)]
#[derive(VolatileFieldAccess)]
#[repr(C)]
pub struct Config {
    /// This is the name associated with this file system.  The tag is
    /// encoded in UTF-8 and padded with NUL bytes if shorter than the
    /// available space.  This field is not NUL-terminated if the encoded bytes
    /// take up the entire field.
    #[access(ReadOnly)]
    tag: [u8; 36],

    /// This is the total number of request virtqueues
    /// exposed by the device.  Each virtqueue offers identical functionality and
    /// there are no ordering guarantees between requests made available on
    /// different queues.  Use of multiple queues is intended to increase
    /// performance.
    #[access(ReadOnly)]
    num_request_queues: le32,

    /// This is the minimum number of bytes required for each
    /// buffer in the notification queue.
    #[access(ReadOnly)]
    notify_buf_size: le32,
}
