use crate::arch;
use crate::errno::*;
use crate::syscalls::usleep;
use crate::time::{itimerval, timespec, timeval};

#[allow(non_camel_case_types)]
pub type clockid_t = i32;

pub(crate) const CLOCK_REALTIME: clockid_t = 1;
pub(crate) const CLOCK_PROCESS_CPUTIME_ID: clockid_t = 2;
pub(crate) const CLOCK_THREAD_CPUTIME_ID: clockid_t = 3;
pub(crate) const CLOCK_MONOTONIC: clockid_t = 4;
pub(crate) const TIMER_ABSTIME: i32 = 4;

/// Finds the resolution (or precision) of a clock.
///
/// This function gets the clock resolution of the clock with `clock_id` and stores it in parameter `res`.
/// Returns `0` on success, `-EINVAL` otherwise.
///
/// Supported clocks:
/// - `CLOCK_REALTIME`
/// - `CLOCK_PROCESS_CPUTIME_ID`
/// - `CLOCK_THREAD_CPUTIME_ID`
/// - `CLOCK_MONOTONIC`
#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_clock_getres(clock_id: clockid_t, res: *mut timespec) -> i32 {
	assert!(
		!res.is_null(),
		"sys_clock_getres called with a zero res parameter"
	);
	let result = unsafe { &mut *res };

	match clock_id {
		CLOCK_REALTIME | CLOCK_PROCESS_CPUTIME_ID | CLOCK_THREAD_CPUTIME_ID | CLOCK_MONOTONIC => {
			// All clocks in Hermit have 1 microsecond resolution.
			*result = timespec::from_usec(1);
			0
		}
		_ => {
			debug!("Called sys_clock_getres for unsupported clock {}", clock_id);
			-EINVAL
		}
	}
}

/// Get the current time of a clock.
///
/// Get the current time of the clock with `clock_id` and stores result in parameter `res`.
/// Returns `0` on success, `-EINVAL` otherwise.
///
/// Supported clocks:
/// - `CLOCK_REALTIME`
/// - `CLOCK_MONOTONIC`
#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_clock_gettime(clock_id: clockid_t, tp: *mut timespec) -> i32 {
	assert!(
		!tp.is_null(),
		"sys_clock_gettime called with a zero tp parameter"
	);
	let result = unsafe { &mut *tp };

	match clock_id {
		CLOCK_REALTIME => {
			*result = timespec::from_usec(arch::kernel::systemtime::now_micros() as i64);
			0
		}
		CLOCK_MONOTONIC => {
			*result = timespec::from_usec(arch::processor::get_timer_ticks() as i64);
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

/// Sleep a clock for a specified number of nanoseconds.
///
/// The requested time (in nanoseconds) must be greater than 0 and less than 1,000,000.
///
/// Returns `0` on success, `-EINVAL` otherwise.
///
/// Supported clocks:
/// - `CLOCK_REALTIME`
/// - `CLOCK_MONOTONIC`
#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_clock_nanosleep(
	clock_id: clockid_t,
	flags: i32,
	rqtp: *const timespec,
	_rmtp: *mut timespec,
) -> i32 {
	assert!(
		!rqtp.is_null(),
		"sys_clock_nanosleep called with a zero rqtp parameter"
	);
	let requested_time = unsafe { &*rqtp };
	if requested_time.tv_sec < 0 || requested_time.tv_nsec > 999_999_999 {
		debug!("sys_clock_nanosleep called with an invalid requested time, returning -EINVAL");
		return -EINVAL;
	}

	match clock_id {
		CLOCK_REALTIME | CLOCK_MONOTONIC => {
			let mut microseconds = (requested_time.tv_sec as u64) * 1_000_000
				+ (requested_time.tv_nsec as u64) / 1_000;

			if flags & TIMER_ABSTIME > 0 {
				if clock_id == CLOCK_REALTIME {
					microseconds -= arch::kernel::systemtime::now_micros();
				} else {
					microseconds -= arch::processor::get_timer_ticks();
				}
			}

			usleep(microseconds);
			0
		}
		_ => -EINVAL,
	}
}

#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_clock_settime(_clock_id: clockid_t, _tp: *const timespec) -> i32 {
	// We don't support setting any clocks yet.
	debug!("sys_clock_settime is unimplemented, returning -EINVAL");
	-EINVAL
}

/// Get the system's clock time.
///
/// This function gets the current time based on the wallclock time when booted up, plus current timer ticks.
/// Returns `0` on success, `-EINVAL` otherwise.
///
/// **Parameter `tz` should be set to `0` since tz is obsolete.**
#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_gettimeofday(tp: *mut timeval, tz: usize) -> i32 {
	if let Some(result) = unsafe { tp.as_mut() } {
		// Return the current time based on the wallclock time when we were booted up
		// plus the current timer ticks.
		let microseconds = arch::kernel::systemtime::now_micros();
		*result = timeval::from_usec(microseconds as i64);
	}

	if tz > 0 {
		debug!("The tz parameter in sys_gettimeofday is unimplemented, returning -EINVAL");
		return -EINVAL;
	}

	0
}

#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_setitimer(
	_which: i32,
	_value: *const itimerval,
	_ovalue: *mut itimerval,
) -> i32 {
	debug!("Called sys_setitimer, which is unimplemented and always returns 0");
	0
}
