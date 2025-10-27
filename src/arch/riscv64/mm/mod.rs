pub mod paging;

use crate::mm::{FrameAlloc, PageAlloc, PageRangeAllocator};

pub fn init() {
	unsafe {
		FrameAlloc::init();
	}
	unsafe {
		PageAlloc::init();
	}
	unsafe {
		self::paging::init_page_tables();
	}
}
