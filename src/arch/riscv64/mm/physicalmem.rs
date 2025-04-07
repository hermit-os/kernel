use core::sync::atomic::Ordering;

use free_list::PageRange;

use crate::arch::riscv64::kernel::{get_limit, get_ram_address};
use crate::mm;
use crate::mm::physicalmem::{PHYSICAL_FREE_LIST, TOTAL_MEMORY};

fn detect_from_limits() -> Result<(), ()> {
	let limit = get_limit();
	if limit == 0 {
		return Err(());
	}

	let range = PageRange::new(
		mm::kernel_end_address().as_usize(),
		get_ram_address().as_usize() + limit,
	)
	.unwrap();
	unsafe {
		PHYSICAL_FREE_LIST.lock().deallocate(range).unwrap();
	}
	TOTAL_MEMORY.store(limit, Ordering::Relaxed);

	Ok(())
}

pub fn init() {
	detect_from_limits().unwrap();
}
