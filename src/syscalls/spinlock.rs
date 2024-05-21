use alloc::boxed::Box;

use hermit_sync::{InterruptTicketMutex, InterruptTicketMutexGuard, TicketMutex, TicketMutexGuard};

use crate::errno::*;

pub struct SpinlockContainer<'a> {
	lock: TicketMutex<()>,
	guard: Option<TicketMutexGuard<'a, ()>>,
}

pub struct SpinlockIrqSaveContainer<'a> {
	lock: InterruptTicketMutex<()>,
	guard: Option<InterruptTicketMutexGuard<'a, ()>>,
}

#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_spinlock_init(lock: *mut *mut SpinlockContainer<'_>) -> i32 {
	if lock.is_null() {
		return -EINVAL;
	}

	let boxed_container = Box::new(SpinlockContainer {
		lock: TicketMutex::new(()),
		guard: None,
	});
	unsafe {
		*lock = Box::into_raw(boxed_container);
	}
	0
}

#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_spinlock_destroy(lock: *mut SpinlockContainer<'_>) -> i32 {
	if lock.is_null() {
		return -EINVAL;
	}

	// Consume the lock into a box, which is then dropped.
	unsafe {
		drop(Box::from_raw(lock));
	}
	0
}

#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_spinlock_lock(lock: *mut SpinlockContainer<'_>) -> i32 {
	if lock.is_null() {
		return -EINVAL;
	}

	let container = unsafe { &mut *lock };
	assert!(
		container.guard.is_none(),
		"Called sys_spinlock_lock when a lock is already held!"
	);
	container.guard = Some(container.lock.lock());
	0
}

#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_spinlock_unlock(lock: *mut SpinlockContainer<'_>) -> i32 {
	if lock.is_null() {
		return -EINVAL;
	}

	let container = unsafe { &mut *lock };
	assert!(
		container.guard.is_some(),
		"Called sys_spinlock_unlock when no lock is currently held!"
	);
	container.guard = None;
	0
}

#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_spinlock_irqsave_init(
	lock: *mut *mut SpinlockIrqSaveContainer<'_>,
) -> i32 {
	if lock.is_null() {
		return -EINVAL;
	}

	let boxed_container = Box::new(SpinlockIrqSaveContainer {
		lock: InterruptTicketMutex::new(()),
		guard: None,
	});
	unsafe {
		*lock = Box::into_raw(boxed_container);
	}
	0
}

#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_spinlock_irqsave_destroy(
	lock: *mut SpinlockIrqSaveContainer<'_>,
) -> i32 {
	if lock.is_null() {
		return -EINVAL;
	}

	// Consume the lock into a box, which is then dropped.
	unsafe {
		drop(Box::from_raw(lock));
	}
	0
}

#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_spinlock_irqsave_lock(lock: *mut SpinlockIrqSaveContainer<'_>) -> i32 {
	if lock.is_null() {
		return -EINVAL;
	}

	let container = unsafe { &mut *lock };
	assert!(
		container.guard.is_none(),
		"Called sys_spinlock_irqsave_lock when a lock is already held!"
	);
	container.guard = Some(container.lock.lock());
	0
}

#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_spinlock_irqsave_unlock(
	lock: *mut SpinlockIrqSaveContainer<'_>,
) -> i32 {
	if lock.is_null() {
		return -EINVAL;
	}

	let container = unsafe { &mut *lock };
	assert!(
		container.guard.is_some(),
		"Called sys_spinlock_irqsave_unlock when no lock is currently held!"
	);
	container.guard = None;
	0
}
