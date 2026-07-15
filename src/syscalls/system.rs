use crate::arch::mm::paging::{BasePageSize, PageSize};

/// Returns the base page size, in bytes, of the current system.
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_getpagesize() -> i32 {
	BasePageSize::SIZE.try_into().unwrap()
}

#[cfg(all(target_arch = "x86_64", feature = "pc-keyboard"))]
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_read_keyboard() -> u8 {
	crate::kernel::pc_keyboard::pop_scancode().unwrap_or(0)
}
