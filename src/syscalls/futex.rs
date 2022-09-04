use core::sync::atomic::AtomicU32;

use crate::{
	errno::EINVAL,
	synch::futex::{self as synch, Flags},
	timespec, timespec_to_microseconds,
};

/// Like `synch::futex_wait`, but does extra sanity checks and takes a `timespec`.
///
/// Returns -EINVAL if
/// * `address` is null
/// * `timeout` is negative
/// * `flags` contains unknown flags
extern "C" fn __sys_futex_wait(
	address: *mut u32,
	expected: u32,
	timeout: *const timespec,
	flags: u32,
) -> i32 {
	if address.is_null() {
		return -EINVAL;
	}

	let address = unsafe { &*(address as *const AtomicU32) };
	let timeout = if timeout.is_null() {
		None
	} else {
		match timespec_to_microseconds(unsafe { timeout.read() }) {
			t @ Some(_) => t,
			None => return -EINVAL,
		}
	};
	let flags = match Flags::from_bits(flags) {
		Some(flags) => flags,
		None => return -EINVAL,
	};

	synch::futex_wait(address, expected, timeout, flags)
}

#[no_mangle]
pub extern "C" fn sys_futex_wait(
	address: *mut u32,
	expected: u32,
	timeout: *const timespec,
	flags: u32,
) -> i32 {
	kernel_function!(__sys_futex_wait(address, expected, timeout, flags))
}

/// Like `synch::futex_wake`, but does extra sanity checks.
///
/// Returns -EINVAL if `address` is null.
extern "C" fn __sys_futex_wake(address: *mut u32, count: i32) -> i32 {
	if address.is_null() {
		return -EINVAL;
	}

	let address = unsafe { &*(address as *const AtomicU32) };
	synch::futex_wake(address, count)
}

#[no_mangle]
pub extern "C" fn sys_futex_wake(address: *mut u32, count: i32) -> i32 {
	kernel_function!(__sys_futex_wake(address, count))
}
