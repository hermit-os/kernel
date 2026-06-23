use crate::arch::mm::paging::{BasePageSize, PageSize};

/// Returns the base page size, in bytes, of the current system.
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_getpagesize() -> i32 {
	BasePageSize::SIZE.try_into().unwrap()
}

/// Returns the address of the framebuffer, if available. Returns 0 if no framebuffer is available.
#[cfg(all(target_arch = "x86_64", feature = "bga"))]
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_get_framebuffer() -> u64 {
	crate::kernel::bga::get_framebuffer_address()
}

#[cfg(not(all(target_arch = "x86_64", feature = "bga")))]
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_get_framebuffer() -> u64 {
	0
}
