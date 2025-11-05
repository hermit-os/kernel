//! A virtio-fs driver.
//!
//! For details on the device, see [File System Device].
//! For details on the Rust definitions, see [`virtio::fs`].
//!
//! [File System Device]: https://docs.oasis-open.org/virtio/virtio/v1.2/cs01/virtio-v1.2-cs01.html#x1-45800011

#[cfg(feature = "pci")]
pub mod virtio_fs;
#[cfg(feature = "pci")]
pub mod virtio_pci;
