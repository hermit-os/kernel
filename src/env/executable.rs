use core::ops::Range;
use core::ptr;

pub fn executable_ptr_range() -> Range<*mut ()> {
	let Range { start, end } = super::boot_info().load_info.kernel_image_addr_range;
	let start = ptr::with_exposed_provenance_mut(start as usize);
	let end = ptr::with_exposed_provenance_mut(end as usize);
	start..end
}
