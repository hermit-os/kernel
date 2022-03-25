use crate::arch;
use crate::arch::percore::*;
use crate::console;
use crate::synch::spinlock::SpinlockIrqSaveGuard;

/// Enables lwIP's printf to print a whole string without being interrupted by
/// a message from the kernel.
static mut CONSOLE_GUARD: Option<SpinlockIrqSaveGuard<'_, console::Console>> = None;

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
	unsafe {
		assert!(CONSOLE_GUARD.is_none());
		CONSOLE_GUARD = Some(console::CONSOLE.lock());
	}
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
	unsafe {
		assert!(CONSOLE_GUARD.is_some());
		drop(CONSOLE_GUARD.take());
	}
}

#[no_mangle]
pub extern "C" fn sys_release_putchar_lock() {
	kernel_function!(__sys_release_putchar_lock())
}
