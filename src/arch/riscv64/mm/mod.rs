pub mod paging;

pub use self::paging::init_page_tables;
use crate::mm::{FrameAlloc, PageAlloc, PageRangeAllocator};

pub fn init() {
	unsafe {
		FrameAlloc::init();
	}
	unsafe {
		PageAlloc::init();
	}
}
