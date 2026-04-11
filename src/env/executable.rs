use core::ops::Range;

use memory_addresses::VirtAddr;

pub fn get_base_address() -> VirtAddr {
	VirtAddr::new(super::boot_info().load_info.kernel_image_addr_range.start)
}

pub fn get_image_size() -> usize {
	let Range { start, end } = super::boot_info().load_info.kernel_image_addr_range;
	(end - start) as usize
}
