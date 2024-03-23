use hermit_sync::{InterruptTicketMutexGuard, SpinMutex};
use lock_api::MutexGuard;

use crate::arch::core_local::core_scheduler;
use crate::{arch, console};

#[hermit_macro::system]
pub extern "C" fn sys_lwip_get_errno() -> i32 {
	core_scheduler().get_lwip_errno()
}

#[hermit_macro::system]
pub extern "C" fn sys_lwip_set_errno(errno: i32) {
	core_scheduler().set_lwip_errno(errno);
}

#[hermit_macro::system]
pub extern "C" fn sys_acquire_putchar_lock() {
	// FIXME: use core-local storage instead
	// better yet: remove and replace all of this
	MutexGuard::leak(console::CONSOLE.lock());
}

#[hermit_macro::system]
pub extern "C" fn sys_putchar(character: u8) {
	arch::output_message_buf(&[character]);
}

#[hermit_macro::system]
pub unsafe extern "C" fn sys_release_putchar_lock() {
	unsafe {
		console::CONSOLE.force_unlock();
	}
}
