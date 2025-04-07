use core::sync::atomic::Ordering;

use free_list::PageRange;

use crate::arch::aarch64::kernel::get_limit;
use crate::mm;
use crate::mm::physicalmem::{PHYSICAL_FREE_LIST, TOTAL_MEMORY};

fn detect_from_limits() -> Result<(), ()> {
	let limit = get_limit();
	if limit == 0 {
		return Err(());
	}

	let range = PageRange::new(mm::kernel_end_address().as_usize(), limit).unwrap();
	TOTAL_MEMORY.store(
		limit - mm::kernel_end_address().as_usize(),
		Ordering::Relaxed,
	);
	unsafe {
		PHYSICAL_FREE_LIST.lock().deallocate(range).unwrap();
	}

	Ok(())
}

pub fn init() {
	detect_from_limits().expect("Unable to determine physical address space!");
}

pub fn init_page_tables() {}
