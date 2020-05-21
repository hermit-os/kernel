// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::arch;
use crate::errno::*;
use crate::syscalls::__sys_usleep;

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct itimerval {
	pub it_interval: timeval,
	pub it_value: timeval,
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct timespec {
	pub tv_sec: i64,
	pub tv_nsec: i64,
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct timeval {
	pub tv_sec: i64,
	pub tv_usec: i64,
}

pub const CLOCK_REALTIME: u64 = 1;
pub const CLOCK_PROCESS_CPUTIME_ID: u64 = 2;
pub const CLOCK_THREAD_CPUTIME_ID: u64 = 3;
pub const CLOCK_MONOTONIC: u64 = 4;
pub const TIMER_ABSTIME: i32 = 4;

fn microseconds_to_timespec(microseconds: u64, result: &mut timespec) {
	result.tv_sec = (microseconds / 1_000_000) as i64;
	result.tv_nsec = ((microseconds % 1_000_000) * 1000) as i64;
}

fn microseconds_to_timeval(microseconds: u64, result: &mut timeval) {
	result.tv_sec = (microseconds / 1_000_000) as i64;
	result.tv_usec = (microseconds % 1_000_000) as i64;
}

fn __sys_clock_getres(clock_id: u64, res: *mut timespec) -> i32 {
	assert!(
		!res.is_null(),
		"sys_clock_getres called with a zero res parameter"
	);
	let result = unsafe { &mut *res };

	match clock_id {
		CLOCK_REALTIME | CLOCK_PROCESS_CPUTIME_ID | CLOCK_THREAD_CPUTIME_ID | CLOCK_MONOTONIC => {
			// All clocks in HermitCore have 1 microsecond resolution.
			microseconds_to_timespec(1, result);
			0
		}
		_ => {
			debug!("Called sys_clock_getres for unsupported clock {}", clock_id);
			-EINVAL
		}
	}
}

#[no_mangle]
pub extern "C" fn sys_clock_getres(clock_id: u64, res: *mut timespec) -> i32 {
	kernel_function!(__sys_clock_getres(clock_id, res))
}

fn __sys_clock_gettime(clock_id: u64, tp: *mut timespec) -> i32 {
	assert!(
		!tp.is_null(),
		"sys_clock_gettime called with a zero tp parameter"
	);
	let result = unsafe { &mut *tp };

	match clock_id {
		CLOCK_REALTIME | CLOCK_MONOTONIC => {
			let mut microseconds = arch::processor::get_timer_ticks();

			if clock_id == CLOCK_REALTIME {
				microseconds += arch::get_boot_time();
			}

			microseconds_to_timespec(microseconds, result);
			0
		}
		_ => {
			debug!(
				"Called sys_clock_gettime for unsupported clock {}",
				clock_id
			);
			-EINVAL
		}
	}
}

#[no_mangle]
pub extern "C" fn sys_clock_gettime(clock_id: u64, tp: *mut timespec) -> i32 {
	kernel_function!(__sys_clock_gettime(clock_id, tp))
}

fn __sys_clock_nanosleep(
	clock_id: u64,
	flags: i32,
	rqtp: *const timespec,
	_rmtp: *mut timespec,
) -> i32 {
	assert!(
		!rqtp.is_null(),
		"sys_clock_nanosleep called with a zero rqtp parameter"
	);
	let requested_time = unsafe { &*rqtp };
	if requested_time.tv_sec < 0
		|| requested_time.tv_nsec < 0
		|| requested_time.tv_nsec > 999_999_999
	{
		debug!("sys_clock_nanosleep called with an invalid requested time, returning -EINVAL");
		return -EINVAL;
	}

	match clock_id {
		CLOCK_REALTIME | CLOCK_MONOTONIC => {
			let mut microseconds = (requested_time.tv_sec as u64) * 1_000_000
				+ (requested_time.tv_nsec as u64) / 1_000;

			if flags & TIMER_ABSTIME > 0 {
				microseconds -= arch::processor::get_timer_ticks();

				if clock_id == CLOCK_REALTIME {
					microseconds -= arch::get_boot_time();
				}
			}

			__sys_usleep(microseconds);
			0
		}
		_ => -EINVAL,
	}
}

#[no_mangle]
pub extern "C" fn sys_clock_nanosleep(
	clock_id: u64,
	flags: i32,
	rqtp: *const timespec,
	rmtp: *mut timespec,
) -> i32 {
	kernel_function!(__sys_clock_nanosleep(clock_id, flags, rqtp, rmtp))
}

fn __sys_clock_settime(_clock_id: u64, _tp: *const timespec) -> i32 {
	// We don't support setting any clocks yet.
	debug!("sys_clock_settime is unimplemented, returning -EINVAL");
	-EINVAL
}

#[no_mangle]
pub extern "C" fn sys_clock_settime(clock_id: u64, tp: *const timespec) -> i32 {
	kernel_function!(__sys_clock_settime(clock_id, tp))
}

fn __sys_gettimeofday(tp: *mut timeval, tz: usize) -> i32 {
	if let Some(result) = unsafe { tp.as_mut() } {
		// Return the current time based on the wallclock time when we were booted up
		// plus the current timer ticks.
		let microseconds = arch::get_boot_time() + arch::processor::get_timer_ticks();
		microseconds_to_timeval(microseconds, result);
	}

	if tz > 0 {
		debug!("The tz parameter in sys_gettimeofday is unimplemented, returning -EINVAL");
		return -EINVAL;
	}

	0
}

#[no_mangle]
pub extern "C" fn sys_gettimeofday(tp: *mut timeval, tz: usize) -> i32 {
	kernel_function!(__sys_gettimeofday(tp, tz))
}

#[no_mangle]
fn __sys_setitimer(_which: i32, _value: *const itimerval, _ovalue: *mut itimerval) -> i32 {
	debug!("Called sys_setitimer, which is unimplemented and always returns 0");
	0
}

#[no_mangle]
pub extern "C" fn sys_setitimer(
	which: i32,
	value: *const itimerval,
	ovalue: *mut itimerval,
) -> i32 {
	kernel_function!(__sys_setitimer(which, value, ovalue))
}
