#[cfg(feature = "virtio-console")]
pub(crate) use crate::arch::kernel::mmio::get_console_driver;
#[cfg(feature = "virtio-entropy")]
pub(crate) use crate::arch::kernel::mmio::get_entropy_driver;
#[cfg(feature = "virtio-fs")]
pub(crate) use crate::arch::kernel::mmio::get_filesystem_driver;
#[cfg(feature = "virtio-vsock")]
pub(crate) use crate::arch::kernel::mmio::get_vsock_driver;
