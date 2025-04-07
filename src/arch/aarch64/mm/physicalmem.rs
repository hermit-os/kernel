use free_list::PageRange;

use crate::arch::aarch64::kernel::get_limit;
use crate::mm;

fn detect_from_limits() -> Result<(), ()> {
	let limit = get_limit();
	if limit == 0 {
		return Err(());
	}

	let range = PageRange::new(mm::kernel_end_address().as_usize(), limit).unwrap();
	unsafe {
		mm::physicalmem::init_frame_range(range);
	}

	Ok(())
}

pub fn init() {
	detect_from_limits().expect("Unable to determine physical address space!");
}

pub fn init_page_tables() {}
