pub mod paging;

pub use self::paging::init_page_tables;

pub fn init() {
	crate::mm::physicalmem::init();
	crate::mm::virtualmem::init();
}
