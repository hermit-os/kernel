// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::arch;
use crate::errno::*;
use crate::synch::semaphore::Semaphore;
use alloc::boxed::Box;

fn __sys_sem_init(sem: *mut *mut Semaphore, value: u32) -> i32 {
	if sem.is_null() {
		return -EINVAL;
	}

	// Create a new boxed semaphore and return a pointer to the raw memory.
	let boxed_semaphore = Box::new(Semaphore::new(value as isize));
	unsafe {
		*sem = Box::into_raw(boxed_semaphore);
	}
	0
}

#[no_mangle]
pub extern "C" fn sys_sem_init(sem: *mut *mut Semaphore, value: u32) -> i32 {
	kernel_function!(__sys_sem_init(sem, value))
}

fn __sys_sem_destroy(sem: *mut Semaphore) -> i32 {
	if sem.is_null() {
		return -EINVAL;
	}

	// Consume the pointer to the raw memory into a Box again
	// and drop the Box to free the associated memory.
	unsafe {
		Box::from_raw(sem);
	}
	0
}

#[no_mangle]
pub extern "C" fn sys_sem_destroy(sem: *mut Semaphore) -> i32 {
	kernel_function!(__sys_sem_destroy(sem))
}

fn __sys_sem_post(sem: *const Semaphore) -> i32 {
	if sem.is_null() {
		return -EINVAL;
	}

	// Get a reference to the given semaphore and release it.
	let semaphore = unsafe { &*sem };
	semaphore.release();
	0
}

#[no_mangle]
pub extern "C" fn sys_sem_post(sem: *const Semaphore) -> i32 {
	kernel_function!(__sys_sem_post(sem))
}

fn __sys_sem_trywait(sem: *const Semaphore) -> i32 {
	if sem.is_null() {
		return -EINVAL;
	}

	// Get a reference to the given semaphore and acquire it in a non-blocking fashion.
	let semaphore = unsafe { &*sem };
	if semaphore.try_acquire() {
		0
	} else {
		-ECANCELED
	}
}

#[no_mangle]
pub extern "C" fn sys_sem_trywait(sem: *const Semaphore) -> i32 {
	kernel_function!(__sys_sem_trywait(sem))
}

fn __sys_sem_timedwait(sem: *const Semaphore, ms: u32) -> i32 {
	if sem.is_null() {
		return -EINVAL;
	}

	let delay = if ms > 0 {
		Some(u64::from(ms))
	} else {
		None
	};

	// Get a reference to the given semaphore and wait until we have acquired it or the wakeup time has elapsed.
	let semaphore = unsafe { &*sem };
	if semaphore.acquire(delay) {
		0
	} else {
		-ETIME
	}
}

#[no_mangle]
pub extern "C" fn sys_sem_timedwait(sem: *const Semaphore, ms: u32) -> i32 {
	kernel_function!(__sys_sem_timedwait(sem, ms))
}

fn __sys_sem_cancelablewait(sem: *const Semaphore, ms: u32) -> i32 {
	sys_sem_timedwait(sem, ms)
}

#[no_mangle]
pub extern "C" fn sys_sem_cancelablewait(sem: *const Semaphore, ms: u32) -> i32 {
	kernel_function!(__sys_sem_cancelablewait(sem, ms))
}
