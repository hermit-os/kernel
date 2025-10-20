pub(crate) const KERNEL_STACK_SIZE: usize = 0x8000;

pub const DEFAULT_STACK_SIZE: usize = 0x0001_0000;

pub(crate) const USER_STACK_SIZE: usize = 0x0010_0000;

#[cfg(any(
	all(any(feature = "tcp", feature = "udp"), not(feature = "rtl8139")),
	feature = "fuse",
	feature = "vsock",
	feature = "balloon"
))]
pub(crate) const VIRTIO_MAX_QUEUE_SIZE: u16 = if cfg!(feature = "pci") { 2048 } else { 1024 };

/// Default keep alive interval in milliseconds
#[cfg(feature = "tcp")]
pub(crate) const DEFAULT_KEEP_ALIVE_INTERVAL: u64 = 75000;

#[cfg(feature = "vsock")]
pub(crate) const VSOCK_PACKET_SIZE: u32 = 8192;
