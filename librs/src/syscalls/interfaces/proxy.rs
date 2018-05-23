// Copyright (c) 2018 Stefan Lankes, RWTH Aachen University
//                    Colin Finck, RWTH Aachen University
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

use syscalls::interfaces::SyscallInterface;
use syscalls::{LWIP_FD_BIT,LWIP_LOCK};
use syscalls::lwip::sys_lwip_get_errno;
use arch::processor::halt;
use core::mem;

extern "C" {
	fn get_proxy_socket() -> i32;
	fn lwip_write(fd: i32, buf: *const u8, len: usize) -> i32;
	fn lwip_read(fd: i32, buf: *mut u8, len: usize) -> i32;
	fn lwip_close(fd: i32) -> i32;
	fn c_strlen(buf: *const u8) -> usize;
}


const HERMIT_MAGIC: i32	= 0x7E317;

const NR_EXIT: i32 = 0;
const NR_WRITE: i32	= 1;
const NR_OPEN: i32 = 2;
const NR_CLOSE: i32 = 3;
const NR_READ: i32 = 4;
const NR_LSEEK: i32 = 5;

static mut LIBC_SD: i32 = -1 as i32;

fn proxy_close()
{
	let _guard = LWIP_LOCK.lock();

	unsafe {
		lwip_close(LIBC_SD);
		LIBC_SD = 0;
	}
}

fn proxy_read<T>(buf: *mut T) {
	let mut len: usize = 0;

	while len < mem::size_of::<T>() {
		unsafe {
			let ret = lwip_read(LIBC_SD, (buf as usize + len) as *mut u8, mem::size_of::<T>()-len);

			if ret > 0 {
				len = len + ret as usize;
			}
		}
	}
}

fn proxy_write<T>(buf: *const T) {
	let mut len: usize = 0;

	while len < mem::size_of::<T>() {
		unsafe {
			let ret = lwip_write(LIBC_SD, (buf as usize + len) as *const u8, mem::size_of::<T>()-len);

			if ret > 0 {
				len = len + ret as usize;
			}
		}
	}
}

fn setup_connection(fd: i32) {
	info!("Setup connection to proxy!");

	unsafe {
		LIBC_SD = fd;
	}

	let mut magic: i32 = 0;
	proxy_read(&mut magic as *mut i32);

	if magic != HERMIT_MAGIC {
		proxy_close();
		panic!("Invalid magic number {}", magic);
	}

	debug!("Receive magic number {}", magic);

	//let mut argc: i32 = 0;
	//proxy_read(&mut argc as *mut i32);
}


//
// TODO: Use a big "tagged union" (aka Rust "enum") with #[repr(C)] for these structures
// once https://github.com/rust-lang/rfcs/blob/master/text/2195-really-tagged-unions.md
// has been implemented.
//
#[repr(C)]
struct SysWrite {
	sysnr: i32,
	fd: i32,
	len: usize
}

impl SysWrite {
	fn new(fd: i32, len: usize) -> SysWrite {
		SysWrite {
			sysnr: NR_WRITE,
			fd: fd,
			len: len
		}
	}
}

#[repr(C)]
struct SysClose {
	sysnr: i32,
	fd: i32
}

impl SysClose {
	fn new(fd: i32) -> SysClose {
		SysClose {
			sysnr: NR_CLOSE,
			fd: fd
		}
	}
}

#[repr(C)]
struct SysOpen {
	sysnr: i32
}

impl SysOpen {
	fn new() -> SysOpen {
		SysOpen {
			sysnr: NR_OPEN
		}
	}
}

#[repr(C)]
struct SysExit {
	sysnr: i32,
	arg: i32
}

impl SysExit {
	fn new(arg: i32) -> SysExit {
		SysExit {
			sysnr: NR_EXIT,
			arg: arg
		}
	}
}

#[repr(C)]
struct SysRead {
	sysnr: i32,
	fd: i32,
	len: usize
}

impl SysRead {
	fn new(fd: i32, len: usize) -> SysRead {
		SysRead {
			sysnr: NR_READ,
			fd: fd,
			len: len
		}
	}
}

#[repr(C)]
struct SysLseek {
	sysnr: i32,
	fd: i32,
	offset: isize,
	whence: i32
}

impl SysLseek {
	fn new(fd: i32, offset: isize, whence: i32) -> SysLseek {
		SysLseek {
			sysnr: NR_LSEEK,
			fd: fd,
			offset: offset,
			whence: whence
		}
	}
}


pub struct Proxy;

impl SyscallInterface for Proxy {
	fn init(&self) {
		let fd = unsafe { get_proxy_socket() };
		assert!(fd >= 0);
		setup_connection(fd);
	}

	fn exit(&self, arg: i32) -> ! {
		let guard = LWIP_LOCK.lock();

		let sysargs = SysExit::new(arg);
		proxy_write(&sysargs as *const SysExit);

		// release lock
		drop(guard);

		loop {
			halt();
		}
	}

	fn open(&self, name: *const u8, flags: i32, mode: i32) -> i32 {
		let _guard = LWIP_LOCK.lock();
		let sysargs = SysOpen::new();
		proxy_write(&sysargs as *const SysOpen);

		let len;
		unsafe { len = c_strlen(name) + 1; }
		proxy_write(&len as *const usize);

		let mut i: usize = 0;
		while i < len {
			let ret;

			unsafe {
				ret = lwip_write(LIBC_SD, (name as usize + i) as *const u8, len-i);
			}

			if ret > 0 {
				i = i + ret as usize;
			}
		}

		proxy_write(&flags as *const i32);
		proxy_write(&mode as *const i32);

		let mut ret: i32 = 0;
		proxy_read(&mut ret as *mut i32);

		ret
	}

	fn close(&self, fd: i32) -> i32 {
		// take lock to protect LwIP
		let _guard = LWIP_LOCK.lock();

		// do we have an LwIP file descriptor?
		if (fd & LWIP_FD_BIT) != 0 {
			let ret;

			unsafe { ret = lwip_close(fd & !LWIP_FD_BIT); }
			if ret < 0 {
				return -sys_lwip_get_errno() as i32;
			}

			return ret as i32;
		}

		let sysargs = SysClose::new(fd);
		proxy_write(&sysargs as *const SysClose);

		let mut ret: i32 = 0;
		proxy_read(&mut ret as *mut i32);

		ret
	}

	fn read(&self, fd: i32, buf: *mut u8, len: usize) -> isize {
		// take lock to protect LwIP
		let _guard = LWIP_LOCK.lock();

		// do we have an LwIP file descriptor?
		if (fd & LWIP_FD_BIT) != 0 {
			let ret;

			unsafe { ret = lwip_read(fd & !LWIP_FD_BIT, buf as *mut u8, len); }
			if ret < 0 {
				return -sys_lwip_get_errno() as isize;
			}

			return ret as isize;
		}

		let sysargs = SysRead::new(fd, len);
		proxy_write(&sysargs as *const SysRead);

		let mut j: isize = 0;
		proxy_read(&mut j as *mut isize);

		if j > 0 {
			let mut i: isize = 0;

			while i < j {
				let ret;

				unsafe {
					ret = lwip_read(LIBC_SD, (buf as isize + i) as *mut u8, (j-i) as usize);
				}

				if ret > 0 {
					i = i + ret as isize;
				}
			}
		}

		j
	}

	fn write(&self, fd: i32, buf: *const u8, len: usize) -> isize {
		// take lock to protect LwIP
		let _guard = LWIP_LOCK.lock();

		// do we have an LwIP file descriptor?
		if (fd & LWIP_FD_BIT) != 0 {
			let ret;

			unsafe { ret = lwip_write(fd & !LWIP_FD_BIT, buf as *const u8, len); }
			if ret < 0 {
				return -sys_lwip_get_errno() as isize;
			}

			return ret as isize;
		}

		let sysargs = SysWrite::new(fd, len);
		proxy_write(&sysargs as *const SysWrite);

		let mut i: usize = 0;

		while i < len {
			let ret;

			unsafe {
				ret = lwip_write(LIBC_SD, (buf as usize + i) as *const u8, len-i);
			}

			if ret > 0 {
				i = i + ret as usize;
			}
		}

		if fd > 2 {
			proxy_read(&mut i as *mut usize);
		}

		i as isize
	}

	fn lseek(&self, fd: i32, offset: isize, whence: i32) -> isize {
		let _guard = LWIP_LOCK.lock();
		let sysargs = SysLseek::new(fd, offset, whence);
		proxy_write(&sysargs as *const SysLseek);

		let mut ret: isize = 0;
		proxy_read(&mut ret as *mut isize);

		ret
	}
}
