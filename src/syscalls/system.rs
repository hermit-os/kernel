#[cfg(all(target_arch = "x86_64", feature = "bga"))]
use core::ffi::c_int;

use crate::arch::mm::paging::{BasePageSize, PageSize};
#[repr(C)]
pub struct FramebufferInfo {
	pub framebuffer: *mut u8,
	pub width: u32,
	pub height: u32,
	pub bpp: u32,
}

/// Returns the base page size, in bytes, of the current system.
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_getpagesize() -> i32 {
	BasePageSize::SIZE.try_into().unwrap()
}

/// Returns the framebuffer information for the BGA device, if it has been initialized. Returns 0
/// on success, or -1 if the BGA device has not been initialized.
#[cfg(all(target_arch = "x86_64", feature = "bga"))]
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_framebuffer(info: *mut FramebufferInfo) -> c_int {
	if info.is_null() {
		return -1;
	};

	let bga_info = crate::arch::kernel::bga::get_framebuffer_info();
	match bga_info {
		Some(bga_info) => {
			let info_c = FramebufferInfo {
				framebuffer: bga_info.framebuffer,
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
