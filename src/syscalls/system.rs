#[hermit_macro::system]
#[no_mangle]
pub extern "C" fn sys_getpagesize() -> i32 {
	crate::arch::mm::paging::get_application_page_size() as i32
}
