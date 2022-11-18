pub mod paging;
pub mod physicalmem;
pub mod virtualmem;

pub use aarch64::paging::{PhysAddr, VirtAddr};

pub use self::physicalmem::init_page_tables;

pub fn init() {
	unsafe {
		paging::init();
	}
	physicalmem::init();
	virtualmem::init();
}
