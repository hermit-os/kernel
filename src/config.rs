#[allow(dead_code)]
pub const KERNEL_STACK_SIZE: usize = 32_768;

#[allow(dead_code)]
pub const DEFAULT_STACK_SIZE: usize = 32_768;

#[allow(dead_code)]
pub const USER_STACK_SIZE: usize = 1_048_576;

#[allow(dead_code)]
pub const VIRTIO_MAX_QUEUE_SIZE: u16 = 2048;

/// See https://github.com/facebook/folly/blob/1b5288e6eea6df074758f877c849b6e73bbb9fbb/folly/lang/Align.h#L107 for details
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub const HW_DESTRUCTIVE_INTERFERENCE_SIZE: usize = 128;
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub const HW_DESTRUCTIVE_INTERFERENCE_SIZE: usize = 64;

use crossbeam_utils::CachePadded;

/// Sanity check for config parameters
pub fn sanity_check() {
	let array = [CachePadded::new(1i8), CachePadded::new(2i8)];
	let addr1 = &*array[0] as *const i8 as usize;
	let addr2 = &*array[1] as *const i8 as usize;

	if HW_DESTRUCTIVE_INTERFERENCE_SIZE != addr2 - addr1 {
		warn!(
			"HW destructive interference size seems to be wrong. Expect false sharing and degraded performance. Should be {}, but is currently set to {}.",
			addr2 - addr1 as usize, HW_DESTRUCTIVE_INTERFERENCE_SIZE
		);
	}
}
