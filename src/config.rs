pub(crate) const KERNEL_STACK_SIZE: usize = 32_768;

pub const DEFAULT_STACK_SIZE: usize = 65_536;

pub(crate) const USER_STACK_SIZE: usize = 1_048_576;

#[allow(dead_code)]
#[cfg(feature = "pci")]
pub(crate) const VIRTIO_MAX_QUEUE_SIZE: u16 = 2048;
#[allow(dead_code)]
#[cfg(not(feature = "pci"))]
pub(crate) const VIRTIO_MAX_QUEUE_SIZE: u16 = 1024;

/// Default keep alive interval in milliseconds
#[cfg(any(feature = "tcp", feature = "udp"))]
pub(crate) const DEFAULT_KEEP_ALIVE_INTERVAL: u64 = 75000;
