// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use arch;
use arch::percore::*;
use console;
use core::u32;
use synch::spinlock::SpinlockIrqSaveGuard;
use syscalls::tasks::Tid;

/// Enables lwIP's printf to print a whole string without being interrupted by
/// a message from the kernel.
static mut CONSOLE_GUARD: Option<SpinlockIrqSaveGuard<console::Console>> = None;

/// Task ID of the single TCP/IP Task spawned by lwIP.
/// Initialized to u32::MAX by default, which is a very unlikely task ID.
static mut LWIP_TCPIP_TASK_ID: Tid = u32::MAX;

pub fn get_lwip_tcpip_task_id() -> Tid {
	unsafe { LWIP_TCPIP_TASK_ID }
}

#[no_mangle]
pub extern "C" fn sys_lwip_register_tcpip_task(id: Tid) {
	unsafe {
		LWIP_TCPIP_TASK_ID = id;
	}
}

#[no_mangle]
pub extern "C" fn sys_lwip_get_errno() -> i32 {
	let lwip_errno = core_scheduler().current_task.borrow().lwip_errno;
	lwip_errno
}

#[no_mangle]
pub extern "C" fn sys_lwip_set_errno(errno: i32) {
	core_scheduler().current_task.borrow_mut().lwip_errno = errno;
}

#[no_mangle]
pub extern "C" fn sys_acquire_putchar_lock() {
	unsafe {
		assert!(CONSOLE_GUARD.is_none());
		CONSOLE_GUARD = Some(console::CONSOLE.lock());
	}
}

#[no_mangle]
pub extern "C" fn sys_putchar(character: u8) {
	arch::output_message_byte(character);
}

#[no_mangle]
pub extern "C" fn sys_release_putchar_lock() {
	unsafe {
		assert!(CONSOLE_GUARD.is_some());
		drop(CONSOLE_GUARD.take());
	}
}
