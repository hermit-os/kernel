//! Alloc syscalls.

use alloc::alloc::{alloc, alloc_zeroed, dealloc, realloc};
use core::alloc::Layout;

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_alloc(size: usize, align: usize) -> *mut u8 {
	unsafe { alloc(layout_from_size_align(size, align)) }
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_dealloc(ptr: *mut u8, size: usize, align: usize) {
	unsafe { dealloc(ptr, layout_from_size_align(size, align)) }
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_alloc_zeroed(size: usize, align: usize) -> *mut u8 {
	unsafe { alloc_zeroed(layout_from_size_align(size, align)) }
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_realloc(
	ptr: *mut u8,
	size: usize,
	align: usize,
	new_size: usize,
) -> *mut u8 {
	unsafe { realloc(ptr, layout_from_size_align(size, align), new_size) }
}

/// Deprecated
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_malloc(size: usize, align: usize) -> *mut u8 {
	unsafe { alloc(layout_from_size_align(size, align)) }
}

/// Deprecated
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_free(ptr: *mut u8, size: usize, align: usize) {
	unsafe { dealloc(ptr, layout_from_size_align(size, align)) }
}

unsafe fn layout_from_size_align(size: usize, align: usize) -> Layout {
	if cfg!(debug_assertions) {
		Layout::from_size_align(size, align).unwrap()
	} else {
		unsafe { Layout::from_size_align_unchecked(size, align) }
	}
}
