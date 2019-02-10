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
use arch::mm::paging;
use scheduler;
use syscalls::interfaces::SyscallInterface;

#[cfg(target_arch = "x86_64")]
use x86::io::*;

const UHYVE_PORT_WRITE: u16 = 0x400;
const UHYVE_PORT_OPEN:	u16 = 0x440;
const UHYVE_PORT_CLOSE:	u16 = 0x480;
const UHYVE_PORT_READ:	u16 = 0x500;
const UHYVE_PORT_EXIT:	u16 = 0x540;
const UHYVE_PORT_LSEEK:	u16 = 0x580;


/// forward a request to the hypervisor uhyve
#[inline]
fn uhyve_send<T>(port: u16, data: &mut T)
{
	let ptr = data as *mut T;
	let physical_address = paging::virtual_to_physical(ptr as usize);

	#[cfg(target_arch = "x86_64")]
	unsafe { outl(port, physical_address as u32); }
}

#[repr(C, packed)]
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

#[repr(C, packed)]
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

#[repr(C, packed)]
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

#[repr(C, packed)]
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
			buf: buf,
			len: len,
			ret: -1
		}
	}
}

#[repr(C, packed)]
struct SysWrite {
	fd: i32,
	buf: *const u8,
	len: usize
}

impl SysWrite {
	fn new(fd: i32, buf: *const u8, len: usize) -> SysWrite {
		SysWrite {
			fd: fd,
			buf: buf,
			len: len
		}
	}
}

#[repr(C, packed)]
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


pub struct Uhyve;

impl SyscallInterface for Uhyve {
	fn open(&self, name: *const u8, flags: i32, mode: i32) -> i32 {
		let mut sysopen = SysOpen::new(name, flags, mode);
		uhyve_send(UHYVE_PORT_OPEN, &mut sysopen);

		sysopen.ret
	}

	fn close(&self, fd: i32) -> i32 {
		let mut sysclose = SysClose::new(fd);
		uhyve_send(UHYVE_PORT_CLOSE, &mut sysclose);

		sysclose.ret
	}

	fn shutdown(&self) -> ! {
		let mut sysexit = SysExit::new(scheduler::get_last_exit_code());
		uhyve_send(UHYVE_PORT_EXIT, &mut sysexit);

		loop {
			arch::processor::halt();
		}
	}

	fn read(&self, fd: i32, buf: *mut u8, len: usize) -> isize {
		// do we have an LwIP file descriptor?
		/*if (fd & LWIP_FD_BIT) != 0 {
			// take lock to protect LwIP
			let _guard = LWIP_LOCK.lock();
			let ret;

			unsafe { ret = lwip_read(fd & !LWIP_FD_BIT, buf as *mut u8, len); }
			if ret < 0 {
				return -sys_lwip_get_errno() as isize;
			}

			return ret as isize;
		}*/

		let mut sysread = SysRead::new(fd, buf, len);
		uhyve_send(UHYVE_PORT_READ, &mut sysread);

		sysread.ret
	}

	fn write(&self, fd: i32, buf: *const u8, len: usize) -> isize {
		// do we have an LwIP file descriptor?
		/*if (fd & LWIP_FD_BIT) != 0 {
			// take lock to protect LwIP
			let _guard = LWIP_LOCK.lock();
			let ret;

			unsafe { ret = lwip_write(fd & !LWIP_FD_BIT, buf as *const u8, len); }
			if ret < 0 {
				return -sys_lwip_get_errno() as isize;
			}

			return ret as isize;
		}*/

		let mut syswrite = SysWrite::new(fd, buf, len);
		uhyve_send(UHYVE_PORT_WRITE, &mut syswrite);

		syswrite.len as isize
	}

	fn lseek(&self, fd: i32, offset: isize, whence: i32) -> isize {
		let mut syslseek = SysLseek::new(fd, offset, whence);
		uhyve_send(UHYVE_PORT_LSEEK, &mut syslseek);

		syslseek.offset
	}
}
