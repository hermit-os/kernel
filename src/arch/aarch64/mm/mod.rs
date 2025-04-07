pub mod paging;
pub mod physicalmem;

pub use self::physicalmem::init_page_tables;

pub fn init() {
	unsafe {
		paging::init();
	}
	physicalmem::init();
	crate::mm::virtualmem::init();
}
