pub mod paging;

use crate::mm::{FrameAlloc, PageAlloc, PageRangeAllocator};

pub unsafe fn init() {
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
