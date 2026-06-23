#[cfg(all(target_arch = "x86_64", feature = "bga"))]
use core::ffi::c_int;

use crate::arch::mm::paging::{BasePageSize, PageSize};

/// Returns the base page size, in bytes, of the current system.
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_getpagesize() -> i32 {
	BasePageSize::SIZE.try_into().unwrap()
}

// Writes the scancodes from the keyboard buffer into the provided buffer.
// If 'nonblock' is true, it will return immediately if there are no scancodes available,
// otherwise it will block until at least one scancode is available.
// Returns the number of bytes written to the buffer,
// or a negative error code on failure.
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
	// SAFETY: We have to trust the user input, because we are a unikernel and if the user wants to crash the program
	// they are free to do so.
	let buffer_slice: &mut [u8] = unsafe { core::slice::from_raw_parts_mut(buffer, size) };
	let result = crate::arch::kernel::pc_keyboard::pop_scancodes(buffer_slice, nonblock);
	if result == 0 && nonblock {
		-(crate::errno::Errno::Again as isize)
	} else {
		result as isize
	}
}
#[cfg(all(target_arch = "x86_64", feature = "bga"))]
#[repr(C)]
pub struct FramebufferInfo {
	pub framebuffer: *mut u8,
	pub width: u32,
	pub height: u32,
	pub bpp: u32,
}

/// Returns the framebuffer information for the video output device.
/// Returns 0 on success, or -1 if the BGA device has not yet been initialized.
#[cfg(all(target_arch = "x86_64", feature = "bga"))]
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_get_framebuffer_info(info: *mut FramebufferInfo) -> c_int {
	if info.is_null() {
		return -1;
	};

	let bga_info = crate::arch::kernel::bga::get_framebuffer_info();
	match bga_info {
		Some(bga_info) => {
			let info_c = FramebufferInfo {
				framebuffer: core::ptr::with_exposed_provenance_mut(
					bga_info.framebuffer.as_usize(),
				),
				width: u32::from(bga_info.width),
				height: u32::from(bga_info.height),
				bpp: u32::from(bga_info.bpp),
			};
			unsafe {
				info.write(info_c);
			}
			0
		}
		None => -1,
	}
}

#[cfg(all(target_arch = "x86_64", feature = "bga"))]
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_set_resolution(width: u16, height: u16, bpp: u16) -> c_int {
	crate::arch::kernel::bga::set_resolution(width, height, bpp);
	0
}
