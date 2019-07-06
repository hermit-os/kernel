// Copyright (c) 2018 Stefan Lankes, RWTH Aachen University
//                    Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

mod generic;
//mod proxy;
mod uhyve;

pub use self::generic::*;
//pub use self::proxy::*;
pub use self::uhyve::*;
use arch;
use console;
use core::fmt::Write;
use core::{isize, slice, str};
use errno::*;

pub trait SyscallInterface: Send + Sync {
	fn init(&self) {
		// Interface-specific initialization steps.
	}

	fn get_application_parameters(&self) -> (i32, *mut *mut u8, *mut *mut u8) {
		let argc = 0;
		let argv = 0 as *mut *mut u8;
		let environ = 0 as *mut *mut u8;

		(argc, argv, environ)
	}

	fn shutdown(&self) -> ! {
		arch::processor::shutdown();
	}

	fn open(&self, _name: *const u8, _flags: i32, _mode: i32) -> i32 {
		debug!("open is unimplemented, returning -ENOSYS");
		-ENOSYS
	}

	fn close(&self, fd: i32) -> i32 {
		// we don't have to close standard descriptors
		if fd < 3 {
			return 0;
		}

		debug!("close is only implemented for stdout & stderr, returning -EINVAL");
		-EINVAL
	}

	fn read(&self, _fd: i32, _buf: *mut u8, _len: usize) -> isize {
		debug!("read is unimplemented, returning -ENOSYS");
		-ENOSYS as isize
	}

	fn write(&self, fd: i32, buf: *const u8, len: usize) -> isize {
		if fd > 2 {
			debug!("write is only implemented for stdout & stderr");
			return -EINVAL as isize;
		}

		assert!(len <= isize::MAX as usize);

		unsafe {
			let slice = slice::from_raw_parts(buf, len);
			console::CONSOLE
				.lock()
				.write_str(str::from_utf8_unchecked(slice))
				.unwrap();
		}

		len as isize
	}

	fn lseek(&self, _fd: i32, _offset: isize, _whence: i32) -> isize {
		debug!("lseek is unimplemented");
		-ENOSYS as isize
	}

	fn stat(&self, _file: *const u8, _st: usize) -> i32 {
		debug!("stat is unimplemented");
		-ENOSYS
	}
}
