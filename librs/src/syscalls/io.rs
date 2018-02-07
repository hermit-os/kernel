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

use arch;
use console;
use core::fmt::Write;
use core::{isize, slice, str};
use errno::*;


#[no_mangle]
pub extern "C" fn sys_open(name: *const u8, flags: i32, mode: i32) -> i32 {
	info!("sys_open is unimplemented, returning -ENOSYS");
	-ENOSYS
}

#[no_mangle]
pub extern "C" fn sys_close(fd: i32) -> i32 {
	info!("sys_close is unimplemented, returning -ENOSYS");
	-ENOSYS
}

#[no_mangle]
pub extern "C" fn sys_read(fd: i32, buf: *mut u8, len: usize) -> isize {
	panic!("sys_read is unimplemented, returning -ENOSYS");
}

#[no_mangle]
pub extern "C" fn sys_write(fd: i32, buf: *const u8, len: usize) -> isize {
	info!("sys_write is halfplemented");
	assert!(len <= isize::MAX as usize);

	unsafe {
		let slice = slice::from_raw_parts(buf, len);
		console::CONSOLE.lock().write_str(str::from_utf8_unchecked(slice)).unwrap();
	}

	len as isize
}

#[no_mangle]
pub extern "C" fn sys_lseek(fd: i32, offset: isize, whence: i32) -> isize {
	panic!("sys_lseek is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_stat(file: *const u8, st: usize) -> i32 {
	-ENOSYS
}

#[no_mangle]
pub extern "C" fn sys_putchar(character: u8) {
	arch::output_message_byte(character);
}
