pub mod paging;

pub fn init() {
	unsafe {
		paging::init();
	}
	crate::mm::physicalmem::init();
	crate::mm::virtualmem::init();
}

pub fn init_page_tables() {}
