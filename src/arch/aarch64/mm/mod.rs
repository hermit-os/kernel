pub mod paging;

use crate::mm::{FrameAlloc, PageAlloc, PageRangeAllocator};

pub fn init() {
	unsafe {
		paging::init();
	}
	unsafe {
		FrameAlloc::init();
	}
	unsafe {
		PageAlloc::init();
	}
}

pub fn init_page_tables() {}
