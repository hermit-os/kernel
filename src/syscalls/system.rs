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
pub unsafe extern "C" fn sys_read_keyboard(buffer: *mut u8, size: usize, nonblock: bool) -> isize {
	if buffer.is_null() {
		return -(crate::errno::Errno::Fault as isize);
	}
	if size == 0 {
		return 0;
	}
	let buffer_slice: &mut [u8] = unsafe { core::slice::from_raw_parts_mut(buffer, size) };
	let result = crate::arch::kernel::pc_keyboard::pop_scancodes(buffer_slice, nonblock);
	if result == 0 && nonblock {
		-(crate::errno::Errno::Again as isize)
	} else {
		result as isize
	}
}
