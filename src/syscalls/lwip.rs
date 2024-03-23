use hermit_sync::{InterruptTicketMutexGuard, SpinMutex};
use lock_api::MutexGuard;

use crate::arch::core_local::core_scheduler;
use crate::{arch, console};

extern "C" fn __sys_lwip_get_errno() -> i32 {
	core_scheduler().get_lwip_errno()
}

#[no_mangle]
pub extern "C" fn sys_lwip_get_errno() -> i32 {
	kernel_function!(__sys_lwip_get_errno())
}

extern "C" fn __sys_lwip_set_errno(errno: i32) {
	core_scheduler().set_lwip_errno(errno);
}

#[no_mangle]
pub extern "C" fn sys_lwip_set_errno(errno: i32) {
	kernel_function!(__sys_lwip_set_errno(errno))
}

extern "C" fn __sys_acquire_putchar_lock() {
	// FIXME: use core-local storage instead
	// better yet: remove and replace all of this
	MutexGuard::leak(console::CONSOLE.lock());
}

#[no_mangle]
pub extern "C" fn sys_acquire_putchar_lock() {
	kernel_function!(__sys_acquire_putchar_lock())
}

extern "C" fn __sys_putchar(character: u8) {
	arch::output_message_buf(&[character]);
}

#[no_mangle]
pub extern "C" fn sys_putchar(character: u8) {
	kernel_function!(__sys_putchar(character))
}

unsafe extern "C" fn __sys_release_putchar_lock() {
	unsafe {
		console::CONSOLE.force_unlock();
	}
}

#[no_mangle]
pub unsafe extern "C" fn sys_release_putchar_lock() {
	unsafe { kernel_function!(__sys_release_putchar_lock()) }
}
