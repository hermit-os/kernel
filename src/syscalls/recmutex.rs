// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::errno::*;
use crate::synch::recmutex::RecursiveMutex;
use alloc::boxed::Box;

fn __sys_recmutex_init(recmutex: *mut *mut RecursiveMutex) -> i32 {
	if recmutex.is_null() {
		return -EINVAL;
	}

	// Create a new boxed recursive mutex and return a pointer to the raw memory.
	let boxed_mutex = Box::new(RecursiveMutex::new());
	unsafe {
		*recmutex = Box::into_raw(boxed_mutex);
	}

	0
}

#[no_mangle]
pub extern "C" fn sys_recmutex_init(recmutex: *mut *mut RecursiveMutex) -> i32 {
	kernel_function!(__sys_recmutex_init(recmutex))
}

fn __sys_recmutex_destroy(recmutex: *mut RecursiveMutex) -> i32 {
	if recmutex.is_null() {
		return -EINVAL;
	}

	// Consume the pointer to the raw memory into a Box again
	// and drop the Box to free the associated memory.
	unsafe {
		Box::from_raw(recmutex);
	}

	0
}

#[no_mangle]
pub extern "C" fn sys_recmutex_destroy(recmutex: *mut RecursiveMutex) -> i32 {
	kernel_function!(__sys_recmutex_destroy(recmutex))
}

fn __sys_recmutex_lock(recmutex: *mut RecursiveMutex) -> i32 {
	if recmutex.is_null() {
		return -EINVAL;
	}

	let mutex = unsafe { &*recmutex };
	mutex.acquire();

	0
}

#[no_mangle]
pub extern "C" fn sys_recmutex_lock(recmutex: *mut RecursiveMutex) -> i32 {
	kernel_function!(__sys_recmutex_lock(recmutex))
}

fn __sys_recmutex_unlock(recmutex: *mut RecursiveMutex) -> i32 {
	if recmutex.is_null() {
		return -EINVAL;
	}

	let mutex = unsafe { &*recmutex };
	mutex.release();

	0
}

#[no_mangle]
pub extern "C" fn sys_recmutex_unlock(recmutex: *mut RecursiveMutex) -> i32 {
	kernel_function!(__sys_recmutex_unlock(recmutex))
}
