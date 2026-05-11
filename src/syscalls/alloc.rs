use core::alloc::{GlobalAlloc, Layout};
use core::ptr;

use crate::mm::ALLOCATOR;

/// Interface to allocate memory from system heap
///
/// # Errors
/// Returning a null pointer indicates that either memory is exhausted or
/// `size` and `align` do not meet this allocator's size or alignment constraints.
///
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_alloc(size: usize, align: usize) -> *mut u8 {
	let layout_res = Layout::from_size_align(size, align);
	if layout_res.is_err() || size == 0 {
		warn!("__sys_alloc called with size {size:#x}, align {align:#x} is an invalid layout!");
		return ptr::null_mut();
	}
	let layout = layout_res.unwrap();
	let ptr = unsafe { ALLOCATOR.alloc(layout) };

	trace!("__sys_alloc: allocate memory at {ptr:p} (size {size:#x}, align {align:#x})");

	ptr
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_alloc_zeroed(size: usize, align: usize) -> *mut u8 {
	let layout_res = Layout::from_size_align(size, align);
	if layout_res.is_err() || size == 0 {
		warn!(
			"__sys_alloc_zeroed called with size {size:#x}, align {align:#x} is an invalid layout!"
		);
		return ptr::null_mut();
	}
	let layout = layout_res.unwrap();
	let ptr = unsafe { ALLOCATOR.alloc_zeroed(layout) };

	trace!("__sys_alloc_zeroed: allocate memory at {ptr:p} (size {size:#x}, align {align:#x})");

	ptr
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_malloc(size: usize, align: usize) -> *mut u8 {
	let layout_res = Layout::from_size_align(size, align);
	if layout_res.is_err() || size == 0 {
		warn!("__sys_malloc called with size {size:#x}, align {align:#x} is an invalid layout!");
		return ptr::null_mut();
	}
	let layout = layout_res.unwrap();
	let ptr = unsafe { ALLOCATOR.alloc(layout) };

	trace!("__sys_malloc: allocate memory at {ptr:p} (size {size:#x}, align {align:#x})");

	ptr
}

/// Shrink or grow a block of memory to the given `new_size`. The block is described by the given
/// ptr pointer and layout. If this returns a non-null pointer, then ownership of the memory block
/// referenced by ptr has been transferred to this allocator. The memory may or may not have been
/// deallocated, and should be considered unusable (unless of course it was transferred back to the
/// caller again via the return value of this method). The new memory block is allocated with
/// layout, but with the size updated to new_size.
/// If this method returns null, then ownership of the memory block has not been transferred to this
/// allocator, and the contents of the memory block are unaltered.
///
/// # Safety
/// This function is unsafe because undefined behavior can result if the caller does not ensure all
/// of the following:
/// - `ptr` must be currently allocated via this allocator,
/// - `size` and `align` must be the same layout that was used to allocate that block of memory.
/// ToDO: verify if the same values for size and align always lead to the same layout
///
/// # Errors
/// Returns null if the new layout does not meet the size and alignment constraints of the
/// allocator, or if reallocation otherwise fails.
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_realloc(
	ptr: *mut u8,
	size: usize,
	align: usize,
	new_size: usize,
) -> *mut u8 {
	unsafe {
		let layout_res = Layout::from_size_align(size, align);
		if layout_res.is_err() || size == 0 || new_size == 0 {
			warn!(
				"__sys_realloc called with ptr {ptr:p}, size {size:#x}, align {align:#x}, new_size {new_size:#x} is an invalid layout!"
			);
			return ptr::null_mut();
		}
		let layout = layout_res.unwrap();
		let new_ptr = ALLOCATOR.realloc(ptr, layout, new_size);

		if new_ptr.is_null() {
			debug!(
				"__sys_realloc failed to resize ptr {ptr:p} with size {size:#x}, align {align:#x}, new_size {new_size:#x} !"
			);
		} else {
			trace!("__sys_realloc: resized memory at {ptr:p}, new address {new_ptr:p}");
		}
		new_ptr
	}
}

/// Interface to deallocate a memory region from the system heap
///
/// # Safety
/// This function is unsafe because undefined behavior can result if the caller does not ensure all of the following:
/// - ptr must denote a block of memory currently allocated via this allocator,
/// - `size` and `align` must be the same values that were used to allocate that block of memory
/// ToDO: verify if the same values for size and align always lead to the same layout
///
/// # Errors
/// May panic if debug assertions are enabled and invalid parameters `size` or `align` where passed.
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_dealloc(ptr: *mut u8, size: usize, align: usize) {
	unsafe {
		let layout_res = Layout::from_size_align(size, align);
		if layout_res.is_err() || size == 0 {
			warn!(
				"__sys_dealloc called with size {size:#x}, align {align:#x} is an invalid layout!"
			);
			debug_assert!(layout_res.is_err(), "__sys_dealloc error: Invalid layout");
			debug_assert_ne!(size, 0, "__sys_dealloc error: size cannot be 0");
		} else {
			trace!("sys_free: deallocate memory at {ptr:p} (size {size:#x})");
		}
		let layout = layout_res.unwrap();
		ALLOCATOR.dealloc(ptr, layout);
	}
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_free(ptr: *mut u8, size: usize, align: usize) {
	unsafe {
		let layout_res = Layout::from_size_align(size, align);
		if layout_res.is_err() || size == 0 {
			warn!("__sys_free called with size {size:#x}, align {align:#x} is an invalid layout!");
			debug_assert!(layout_res.is_err(), "__sys_free error: Invalid layout");
			debug_assert_ne!(size, 0, "__sys_free error: size cannot be 0");
		} else {
			trace!("sys_free: deallocate memory at {ptr:p} (size {size:#x})");
		}
		let layout = layout_res.unwrap();
		ALLOCATOR.dealloc(ptr, layout);
	}
}
