pub(crate) const KERNEL_STACK_SIZE: usize = 32_768;

pub(crate) const DEFAULT_STACK_SIZE: usize = 65_536;

pub(crate) const USER_STACK_SIZE: usize = 1_048_576;

#[cfg(feature = "pci")]
pub(crate) const VIRTIO_MAX_QUEUE_SIZE: u16 = 2048;
#[cfg(not(feature = "pci"))]
pub(crate) const VIRTIO_MAX_QUEUE_SIZE: u16 = 1024;

/// Default keep alive interval in milliseconds
#[cfg(feature = "tcp")]
pub(crate) const DEFAULT_KEEP_ALIVE_INTERVAL: u64 = 75000;

pub(crate) const HW_DESTRUCTIVE_INTERFERENCE_SIZE: usize =
	core::mem::align_of::<crossbeam_utils::CachePadded<u8>>();
