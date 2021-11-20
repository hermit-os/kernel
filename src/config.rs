pub const KERNEL_STACK_SIZE: usize = 32_768;

pub const DEFAULT_STACK_SIZE: usize = 32_768;

pub const USER_STACK_SIZE: usize = 1_048_576;

#[cfg(feature = "pci")]
pub const VIRTIO_MAX_QUEUE_SIZE: u16 = 2048;
#[cfg(not(feature = "pci"))]
pub const VIRTIO_MAX_QUEUE_SIZE: u16 = 1024;

pub const HW_DESTRUCTIVE_INTERFERENCE_SIZE: usize = {
	use core::ptr;
	use crossbeam_utils::CachePadded;

	let array = [CachePadded::new(0_u8); 2];
	let ptr0 = ptr::addr_of!(array[0]).cast::<u8>();
	let ptr1 = ptr::addr_of!(array[1]).cast::<u8>();

	unsafe { ptr1.offset_from(ptr0) as usize }
};
