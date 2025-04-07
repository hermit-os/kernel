pub mod paging;
pub mod physicalmem;

pub use self::paging::init_page_tables;

pub fn init() {
	physicalmem::init();
	crate::mm::virtualmem::init();
}
