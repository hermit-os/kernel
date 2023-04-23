use alloc::boxed::Box;

use crate::errno::*;
use crate::synch::semaphore::Semaphore;

/// Create a new, unnamed semaphore
/// Stores the raw memory location of the new semaphore in parameter sem
/// Returns 0 on success, -EINVAL otherwise
extern "C" fn __sys_sem_init(sem: *mut *mut Semaphore, value: u32) -> i32 {
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

/// Destroy and deallocate a semaphore
/// Returns 0 on success, -EINVAL otherwise
extern "C" fn __sys_sem_destroy(sem: *mut Semaphore) -> i32 {
	if sem.is_null() {
		return -EINVAL;
	}

	// Consume the pointer to the raw memory into a Box again
	// and drop the Box to free the associated memory.
	unsafe {
		drop(Box::from_raw(sem));
	}
	0
}

#[no_mangle]
pub extern "C" fn sys_sem_destroy(sem: *mut Semaphore) -> i32 {
	kernel_function!(__sys_sem_destroy(sem))
}

/// Release a semaphore
/// Returns 0 on success, -EINVAL otherwise
extern "C" fn __sys_sem_post(sem: *const Semaphore) -> i32 {
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

/// Try a decrement (lock) on semaphore sem
/// If the semaphore currently has a value of 0, funciton returns immediately
///
/// Returns 0 on lock acquire, -EINVAL if sem is null, -ECANCELED if decrement fails
extern "C" fn __sys_sem_trywait(sem: *const Semaphore) -> i32 {
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

/// Try a decrement (lock) on semaphore sem
/// Block until semaphore is acquired or until wakeup time has elapsed
///
/// Returns 0 on lock acquire, -EINVAL if sem is null, -ETIME on wait timeout
extern "C" fn __sys_sem_timedwait(sem: *const Semaphore, ms: u32) -> i32 {
	if sem.is_null() {
		return -EINVAL;
	}

	let delay = if ms > 0 { Some(u64::from(ms)) } else { None };

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

extern "C" fn __sys_sem_cancelablewait(sem: *const Semaphore, ms: u32) -> i32 {
	sys_sem_timedwait(sem, ms)
}

#[no_mangle]
pub extern "C" fn sys_sem_cancelablewait(sem: *const Semaphore, ms: u32) -> i32 {
	kernel_function!(__sys_sem_cancelablewait(sem, ms))
}
