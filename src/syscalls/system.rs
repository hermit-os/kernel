use crate::arch;

extern "C" fn __sys_getpagesize() -> i32 {
	arch::mm::paging::get_application_page_size() as i32
}

#[no_mangle]
pub extern "C" fn sys_getpagesize() -> i32 {
	kernel_function!(__sys_getpagesize())
}
