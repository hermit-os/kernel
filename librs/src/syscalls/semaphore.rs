// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// MIT License
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
// WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

use alloc::boxed::Box;
use errno::*;
use synch::semaphore::Semaphore;


#[no_mangle]
pub extern "C" fn sys_sem_init(sem: *mut *mut Semaphore, value: u32) -> i32 {
	if sem.is_null() {
		return -EINVAL;
	}

	let boxed_semaphore = Box::new(Semaphore::new(value as isize));
	unsafe { *sem = Box::into_raw(boxed_semaphore); }
	0
}

#[no_mangle]
pub extern "C" fn sys_sem_destroy(sem: *mut Semaphore) -> i32 {
	if sem.is_null() {
		return -EINVAL;
	}

	unsafe { Box::from_raw(sem); }
	0
}

#[no_mangle]
pub extern "C" fn sys_sem_wait(sem: *const Semaphore) -> i32 {
	if sem.is_null() {
		return -EINVAL;
	}

	let semaphore = unsafe { & *sem };
	semaphore.acquire();
	0
}

#[no_mangle]
pub extern "C" fn sys_sem_post(sem: *const Semaphore) -> i32 {
	if sem.is_null() {
		return -EINVAL;
	}

	let semaphore = unsafe { & *sem };
	semaphore.release();
	0
}

#[no_mangle]
pub extern "C" fn sys_sem_trywait(sem: *const Semaphore) -> i32 {
	if sem.is_null() {
		return -EINVAL;
	}

	let semaphore = unsafe { & *sem };
	if semaphore.try_acquire() {
		0
	} else {
		-ECANCELED
	}
}

#[no_mangle]
pub extern "C" fn sys_sem_timedwait(sem: *const Semaphore, ms: u32) -> i32 {
	panic!("sys_sem_timedwait is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_sem_cancelablewait(sem: *const Semaphore, ms: u32) -> i32 {
	panic!("sys_sem_cancelablewait is unimplemented");
}
