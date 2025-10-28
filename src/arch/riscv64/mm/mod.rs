pub mod paging;

use crate::mm::{FrameAlloc, PageAlloc, PageRangeAllocator};

pub unsafe fn init() {
	unsafe {
		FrameAlloc::init();
	}
	unsafe {
		PageAlloc::init();
	}
	unsafe {
		self::paging::enable_page_table();
	}
}
