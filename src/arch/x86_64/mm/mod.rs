pub(crate) mod paging;

use memory_addresses::arch::x86_64::{PhysAddr, VirtAddr};

use crate::mm::{FrameAlloc, PageAlloc, PageRangeAllocator};

pub unsafe fn init() {
	unsafe {
		paging::init();
	}
	unsafe {
		FrameAlloc::init();
	}
	unsafe {
		paging::log_page_tables();
	}
	unsafe {
		PageAlloc::init();
	}
}
