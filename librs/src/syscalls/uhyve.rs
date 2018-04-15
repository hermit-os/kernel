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

use syscalls::syscalls::SyscallInterface;
use syscalls::{LWIP_FD_BIT,LWIP_LOCK};
use syscalls::lwip::sys_lwip_get_errno;
use arch::mm::paging;
use arch::processor::halt;
use arch::uhyve_send;

const UHYVE_PORT_WRITE: u16 = 0x400;
const UHYVE_PORT_OPEN:	u16 = 0x440;
const UHYVE_PORT_CLOSE:	u16 = 0x480;
const UHYVE_PORT_READ:	u16 = 0x500;
const UHYVE_PORT_EXIT:	u16 = 0x540;
const UHYVE_PORT_LSEEK:	u16 = 0x580;

extern "C" {
	fn lwip_write(fd: i32, buf: *const u8, len: usize) -> i32;
	fn lwip_read(fd: i32, buf: *mut u8, len: usize) -> i32;
}

pub struct Uhyve;

impl Uhyve {
	pub const fn new() -> Uhyve {
		Uhyve {}
	}
}

#[repr(C)]
struct SysExit {
	arg: i32,
}

impl SysExit {
	fn new(arg: i32) -> SysExit {
		SysExit {
			arg: arg
		}
	}
}

#[repr(C)]
struct SysOpen {
	name: *const u8,
	flags: i32,
	mode: i32,
	ret: i32
}

impl SysOpen {
	fn new(name: *const u8, flags: i32, mode: i32) -> SysOpen {
		SysOpen {
			name: paging::virtual_to_physical(name as usize) as *const u8,
			flags: flags,
			mode: mode,
			ret: -1
		}
	}
}

#[repr(C)]
struct SysClose {
	fd: i32,
	ret: i32
}

impl SysClose {
	fn new(fd: i32) -> SysClose {
		SysClose {
			fd: fd,
			ret: -1
		}
	}
}

#[repr(C)]
struct SysRead {
	fd: i32,
	buf: *const u8,
	len: usize,
	ret: isize
}

impl SysRead {
	fn new(fd: i32, buf: *const u8, len: usize) -> SysRead {
		SysRead {
			fd: fd,
			buf: paging::virtual_to_physical(buf as usize) as *const u8,
			len: len,
			ret: -1
		}
	}
}

#[repr(C)]
struct SysWrite {
	fd: i32,
	buf: *const u8,
	len: usize
}

impl SysWrite {
	fn new(fd: i32, buf: *const u8, len: usize) -> SysWrite {
		SysWrite {
			fd: fd,
			buf: paging::virtual_to_physical(buf as usize) as *const u8,
			len: len
		}
	}
}

#[repr(C)]
struct SysLseek {
	fd: i32,
	offset: isize,
	whence: i32
}

impl SysLseek {
	fn new(fd: i32, offset: isize, whence: i32) -> SysLseek {
		SysLseek {
			fd: fd,
			offset: offset,
			whence: whence
		}
	}
}

impl SyscallInterface for Uhyve {
	fn open(&self, name: *const u8, flags: i32, mode: i32) -> i32 {
		let mut sysopen = SysOpen::new(name, flags, mode);
		let raw_mut = &mut sysopen as *mut SysOpen;

		uhyve_send(UHYVE_PORT_OPEN, paging::virtual_to_physical(raw_mut as usize));

		sysopen.ret
	}

	fn close(&self, fd: i32) -> i32 {
		let mut sysclose = SysClose::new(fd);
		let raw_mut = &mut sysclose as *mut SysClose;

		uhyve_send(UHYVE_PORT_CLOSE, paging::virtual_to_physical(raw_mut as usize));

		sysclose.ret
	}

	fn exit(&self, arg: i32) -> ! {
		let mut sysexit = SysExit::new(arg);
		let raw_mut = &mut sysexit as *mut SysExit;

		uhyve_send(UHYVE_PORT_EXIT, paging::virtual_to_physical(raw_mut as usize));

		loop {
			halt();
		}
	}

	fn read(&self, fd: i32, buf: *mut u8, len: usize) -> isize {
		// do we have an LwIP file descriptor?
		if (fd & LWIP_FD_BIT) != 0 {
			// take lock to protect LwIP
			let _guard = LWIP_LOCK.lock();
			let ret;

			unsafe { ret = lwip_read(fd & !LWIP_FD_BIT, buf as *mut u8, len); }
			if ret < 0 {
				return -sys_lwip_get_errno() as isize;
			}

			return ret as isize;
		}

		let mut sysread = SysRead::new(fd, buf, len);
		let raw_mut = &mut sysread as *mut SysRead;

		uhyve_send(UHYVE_PORT_READ, paging::virtual_to_physical(raw_mut as usize));

		sysread.ret
	}

	fn write(&self, fd: i32, buf: *const u8, len: usize) -> isize {
		// do we have an LwIP file descriptor?
		if (fd & LWIP_FD_BIT) != 0 {
			// take lock to protect LwIP
			let _guard = LWIP_LOCK.lock();
			let ret;

			unsafe { ret = lwip_write(fd & !LWIP_FD_BIT, buf as *const u8, len); }
			if ret < 0 {
				return -sys_lwip_get_errno() as isize;
			}

			return ret as isize;
		}

		let mut syswrite = SysWrite::new(fd, buf, len);
		let raw_mut = &mut syswrite as *mut SysWrite;

		uhyve_send(UHYVE_PORT_WRITE, paging::virtual_to_physical(raw_mut as usize));

		syswrite.len as isize
	}

	fn lseek(&self, fd: i32, offset: isize, whence: i32) -> isize {
		let mut syslseek = SysLseek::new(fd, offset, whence);
		let raw_mut = &mut syslseek as *mut SysLseek;

		uhyve_send(UHYVE_PORT_LSEEK, paging::virtual_to_physical(raw_mut as usize));

		syslseek.offset
	}
}
