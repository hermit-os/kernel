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
use synch::spinlock::*;

pub struct SpinlockContainer<'a> {
	lock: Spinlock<()>,
	guard: Option<SpinlockGuard<'a, ()>>,
}

pub struct SpinlockIrqSaveContainer<'a> {
	lock: SpinlockIrqSave<()>,
	guard: Option<SpinlockIrqSaveGuard<'a, ()>>,
}


#[no_mangle]
pub extern "C" fn sys_spinlock_init(lock: *mut *mut SpinlockContainer) -> i32 {
	if lock.is_null() {
		return -EINVAL;
	}

	let boxed_container = Box::new(
		SpinlockContainer {
			lock: Spinlock::new(()),
			guard: None,
		}
	);
	unsafe { *lock = Box::into_raw(boxed_container); }
	0
}

#[no_mangle]
pub extern "C" fn sys_spinlock_destroy(lock: *mut SpinlockContainer) -> i32 {
	if lock.is_null() {
		return -EINVAL;
	}

	// Consume the lock into a box, which is then dropped.
	unsafe { Box::from_raw(lock); }
	0
}

#[no_mangle]
pub extern "C" fn sys_spinlock_lock(lock: *mut SpinlockContainer) -> i32 {
	if lock.is_null() {
		return -EINVAL;
	}

	let container = unsafe { &mut *lock };
	assert!(container.guard.is_none(), "Called sys_spinlock_lock when a lock is already held!");
	container.guard = Some(container.lock.lock());
	0
}

#[no_mangle]
pub extern "C" fn sys_spinlock_unlock(lock: *mut SpinlockContainer) -> i32 {
	if lock.is_null() {
		return -EINVAL;
	}

	let container = unsafe { &mut *lock };
	assert!(container.guard.is_some(), "Called sys_spinlock_unlock when no lock is currently held!");
	container.guard = None;
	0
}


#[no_mangle]
pub extern "C" fn sys_spinlock_irqsave_init(lock: *mut *mut SpinlockIrqSaveContainer) -> i32 {
	if lock.is_null() {
		return -EINVAL;
	}

	let boxed_container = Box::new(
		SpinlockIrqSaveContainer {
			lock: SpinlockIrqSave::new(()),
			guard: None,
		}
	);
	unsafe { *lock = Box::into_raw(boxed_container); }
	0
}

#[no_mangle]
pub extern "C" fn sys_spinlock_irqsave_destroy(lock: *mut SpinlockIrqSaveContainer) -> i32 {
	if lock.is_null() {
		return -EINVAL;
	}

	// Consume the lock into a box, which is then dropped.
	unsafe { Box::from_raw(lock); }
	0
}

#[no_mangle]
pub extern "C" fn sys_spinlock_irqsave_lock(lock: *mut SpinlockIrqSaveContainer) -> i32 {
	if lock.is_null() {
		return -EINVAL;
	}

	let container = unsafe { &mut *lock };
	assert!(container.guard.is_none(), "Called sys_spinlock_irqsave_lock when a lock is already held!");
	container.guard = Some(container.lock.lock());
	0
}

#[no_mangle]
pub extern "C" fn sys_spinlock_irqsave_unlock(lock: *mut SpinlockIrqSaveContainer) -> i32 {
	if lock.is_null() {
		return -EINVAL;
	}

	let container = unsafe { &mut *lock };
	assert!(container.guard.is_some(), "Called sys_spinlock_irqsave_unlock when no lock is currently held!");
	container.guard = None;
	0
}
