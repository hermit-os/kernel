use alloc::boxed::Box;

use crate::errno::*;
use crate::synch::semaphore::Semaphore;
use crate::syscalls::{sys_clock_gettime, CLOCK_REALTIME};
use crate::time::timespec;

#[allow(non_camel_case_types)]
pub type sem_t = *const Semaphore;

/// Create a new, unnamed semaphore.
///
/// This function can be used to get the raw memory location of a semaphore.
///
/// Stores the raw memory location of the new semaphore in parameter `sem`.
/// Returns `0` on success, `-EINVAL` if `sem` is null.
#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_sem_init(sem: *mut sem_t, pshared: i32, value: u32) -> i32 {
	if sem.is_null() || pshared != 0 {
		return -EINVAL;
	}

	// Create a new boxed semaphore and return a pointer to the raw memory.
	let boxed_semaphore = Box::new(Semaphore::new(value as isize));
	unsafe {
		*sem = Box::into_raw(boxed_semaphore);
	}
	0
}

/// Destroy and deallocate a semaphore.
///
/// This function can be used to manually deallocate a semaphore via a reference.
///
/// Returns `0` on success, `-EINVAL` if `sem` is null.
#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_sem_destroy(sem: *mut sem_t) -> i32 {
	if sem.is_null() {
		return -EINVAL;
	}

	// Consume the pointer to the raw memory into a Box again
	// and drop the Box to free the associated memory.
	unsafe {
		drop(Box::from_raw((*sem).cast_mut()));
	}
	0
}

/// Release a semaphore.
///
/// This function can be used to allow the next blocked waiter to access this semaphore.
/// It will notify the next waiter that `sem` is available.
/// The semaphore is not deallocated after being released.
///
/// Returns `0` on success, or `-EINVAL` if `sem` is null.
#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_sem_post(sem: *mut sem_t) -> i32 {
	if sem.is_null() {
		return -EINVAL;
	}

	// Get a reference to the given semaphore and release it.
	let semaphore = unsafe { &**sem };
	semaphore.release();
	0
}

/// Try to acquire a lock on a semaphore.
///
/// This function does not block if the acquire fails.
/// If the acquire fails (i.e. the semaphore's count is already 0), the function returns immediately.
///
/// Returns `0` on lock acquire, `-EINVAL` if `sem` is null, or `-ECANCELED` if the decrement fails.
#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_sem_trywait(sem: *mut sem_t) -> i32 {
	if sem.is_null() {
		return -EINVAL;
	}

	// Get a reference to the given semaphore and acquire it in a non-blocking fashion.
	let semaphore = unsafe { &**sem };
	if semaphore.try_acquire() {
		0
	} else {
		-ECANCELED
	}
}

unsafe fn sem_timedwait(sem: *mut sem_t, ms: u32) -> i32 {
	if sem.is_null() {
		return -EINVAL;
	}

	let delay = if ms > 0 { Some(u64::from(ms)) } else { None };

	// Get a reference to the given semaphore and wait until we have acquired it or the wakeup time has elapsed.
	let semaphore = unsafe { &**sem };
	if semaphore.acquire(delay) {
		0
	} else {
		-ETIME
	}
}

/// Try to acquire a lock on a semaphore.
///
/// Blocks until semaphore is acquired or until specified time passed
///
/// Returns `0` on lock acquire, `-EINVAL` if sem is null, or `-ETIME` on timeout.
#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_sem_timedwait(sem: *mut sem_t, ts: *const timespec) -> i32 {
	if ts.is_null() {
		unsafe { sem_timedwait(sem, 0) }
	} else {
		let mut current_ts: timespec = Default::default();

		unsafe {
			sys_clock_gettime(CLOCK_REALTIME, &mut current_ts as *mut _);

			let ts = &*ts;
			let ms: i64 = (ts.tv_sec - current_ts.tv_sec) * 1000
				+ (ts.tv_nsec as i64 - current_ts.tv_nsec as i64) / 1000000;

			if ms > 0 {
				sem_timedwait(sem, ms.try_into().unwrap())
			} else {
				0
			}
		}
	}
}
