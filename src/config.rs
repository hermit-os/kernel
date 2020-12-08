#[allow(dead_code)]
pub const KERNEL_STACK_SIZE: usize = 32_768;

#[allow(dead_code)]
pub const DEFAULT_STACK_SIZE: usize = 32_768;

#[allow(dead_code)]
pub const USER_STACK_SIZE: usize = 1_048_576;

#[allow(dead_code)]
pub const VIRTIO_MAX_QUEUE_SIZE: u16 = 256;

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub const HW_DESTRUCTIVE_INTERFERENCE_SIZE: usize = 128;
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub const HW_DESTRUCTIVE_INTERFERENCE_SIZE: usize = 64;
