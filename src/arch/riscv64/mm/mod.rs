pub mod paging;
pub mod physicalmem;
pub mod virtualmem;

pub use self::paging::init_page_tables;

pub fn init() {
	physicalmem::init();
	virtualmem::init();
}
