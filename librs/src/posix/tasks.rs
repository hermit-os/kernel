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

use errno::*;
use syscalls::sys_usleep;

extern "C" {
	fn __errno() -> *mut i32;
}

#[repr(C)]
pub struct timespec {
	pub tv_sec: i64,
	pub tv_nsec: i64,
}

#[no_mangle]
pub extern "C" fn nanosleep(rqtp: *const timespec, _rmtp: *mut timespec) -> i32 {
	if rqtp.is_null() {
		unsafe { *__errno() = EINVAL; }
		return -1;
	}

	let requested_time = unsafe { & *rqtp };
	if requested_time.tv_sec < 0 || requested_time.tv_nsec < 0 || requested_time.tv_nsec > 999_999_999 {
		unsafe { *__errno() = EINVAL; }
		return -1;
	}

	let microseconds = (requested_time.tv_sec as u64) * 1_000_000 + (requested_time.tv_nsec as u64) / 1_000;
	sys_usleep(microseconds);
	0
}
