// Copyright (c) 2018 Stefan Lankes, RWTH Aachen University
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

use arch;
use arch::percore::*;
use console;
use core::fmt::Write;
use core::{isize, slice, str};
use errno::*;

pub trait SyscallInterface : Send + Sync {
	fn exit(&self, arg: i32) -> ! {
		core_scheduler().exit(arg);
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
			console::CONSOLE.lock().write_str(str::from_utf8_unchecked(slice)).unwrap();
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

	fn putchar(&self, character: u8) {
		arch::output_message_byte(character);
	}
}
