use hermit_sync::{InterruptTicketMutexGuard, SpinMutex};

use crate::arch::percore::core_scheduler;
use crate::{arch, console};

/// Enables lwIP's printf to print a whole string without being interrupted by
/// a message from the kernel.
static CONSOLE_GUARD: SpinMutex<Option<InterruptTicketMutexGuard<'_, console::Console>>> =
	SpinMutex::new(None);

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
	let mut console_guard = CONSOLE_GUARD.lock();
	assert!(console_guard.is_none());
	*console_guard = Some(console::CONSOLE.lock());
}

#[no_mangle]
pub extern "C" fn sys_acquire_putchar_lock() {
	kernel_function!(__sys_acquire_putchar_lock())
}

extern "C" fn __sys_putchar(character: u8) {
	arch::output_message_byte(character);
}

#[no_mangle]
pub extern "C" fn sys_putchar(character: u8) {
	kernel_function!(__sys_putchar(character))
}

extern "C" fn __sys_release_putchar_lock() {
	let mut console_guard = CONSOLE_GUARD.lock();
	assert!(console_guard.is_some());
	drop(console_guard.take());
}

#[no_mangle]
pub extern "C" fn sys_release_putchar_lock() {
	kernel_function!(__sys_release_putchar_lock())
}
