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

use alloc::vec::Vec;
use arch;
use core::{mem, slice};
use scheduler;
use syscalls::{LWIP_FD_BIT,LWIP_LOCK};
use syscalls::interfaces::SyscallInterface;
use syscalls::lwip::sys_lwip_get_errno;

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

fn proxy_read_bytes(buf: &mut [u8]) {
	let mut bytes_read = 0;

	while bytes_read < buf.len() {
		let slice_remaining = &mut buf[bytes_read..];
		let ptr = slice_remaining.as_mut_ptr();

		let ret = unsafe { lwip_read(LIBC_SD, ptr, slice_remaining.len()) };
		if ret > 0 {
			bytes_read += ret as usize;
		}
	}
}

fn proxy_read_value<T>(buf: &mut T) {
	let byte_pointer = buf as *mut T as usize as *mut u8;
	let byte_slice = unsafe { slice::from_raw_parts_mut(byte_pointer, mem::size_of::<T>()) };
	proxy_read_bytes(byte_slice);
}

fn proxy_read_parameter_vector() -> (i32, *mut *mut u8) {
	// proxy first sends the number of parameters.
	let mut parameter_count: i32 = 0;
	proxy_read_value(&mut parameter_count);

	// Allocate a vector for that many pointers to C strings and iterate through each one.
	let mut parameters = Vec::<*mut u8>::with_capacity(parameter_count as usize + 1);
	for _i in 0..parameter_count {
		// For each parameter, proxy first sends its length.
		let mut parameter_length: i32 = 0;
		proxy_read_value(&mut parameter_length);

		// Allocate a vector for that many characters *and* preallocate it with uninitialized memory.
		let mut parameter = Vec::<u8>::with_capacity(parameter_length as usize);
		unsafe { parameter.set_len(parameter_length as usize); }

		{
			// Turn the vector into a slice and read all characters into it.
			let parameter_slice = parameter.as_mut_slice();
			proxy_read_bytes(parameter_slice);

			// Finally, add this parameter to our parameters vector.
			parameters.push(parameter_slice.as_mut_ptr());
		}

		// Make sure that Rust does not deallocate the memory for this parameter!
		mem::forget(parameter);
	}

	// Add a final null parameter to indicate the end of the parameters list for C applications.
	parameters.push(0 as *mut u8);

	// Get a raw pointer to the parameters vector and make sure that Rust does not deallocate it.
	let parameter_ptr = parameters.as_mut_slice().as_mut_ptr();
	mem::forget(parameters);

	(parameter_count, parameter_ptr)
}

fn proxy_write_bytes(buf: &[u8]) {
	let mut bytes_written = 0;

	while bytes_written < buf.len() {
		let slice_remaining = &buf[bytes_written..];
		let ptr = slice_remaining.as_ptr();

		let ret = unsafe { lwip_write(LIBC_SD, ptr, slice_remaining.len()) };
		if ret > 0 {
			bytes_written += ret as usize;
		}
	}
}

fn proxy_write_value<T>(buf: &T) {
	let byte_pointer = buf as *const T as usize as *const u8;
	let byte_slice = unsafe { slice::from_raw_parts(byte_pointer, mem::size_of::<T>()) };
	proxy_write_bytes(byte_slice);
}


fn setup_connection(fd: i32) {
	info!("Setup connection to proxy!");

	unsafe {
		LIBC_SD = fd;
	}

	let mut magic: i32 = 0;
	proxy_read_value(&mut magic);

	if magic != HERMIT_MAGIC {
		proxy_close();
		panic!("Invalid magic number {}", magic);
	}

	debug!("Received magic number {}", magic);
}


//
// TODO: Use a big "tagged union" (aka Rust "enum") with #[repr(C)] for these structures
// once https://github.com/rust-lang/rfcs/blob/master/text/2195-really-tagged-unions.md
// has been implemented.
//
#[repr(C, packed)]
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

#[repr(C, packed)]
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

#[repr(C, packed)]
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

#[repr(C, packed)]
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

#[repr(C, packed)]
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

#[repr(C, packed)]
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

	fn get_application_parameters(&self) -> (i32, *mut *mut u8, *mut *mut u8) {
		let (argc, argv) = proxy_read_parameter_vector();
		let (_envc, environ) = proxy_read_parameter_vector();

		(argc, argv, environ)
	}

	fn shutdown(&self) -> ! {
		let _guard = LWIP_LOCK.lock();

		let sysargs = SysExit::new(scheduler::get_last_exit_code());
		proxy_write_value(&sysargs);

		loop {
			arch::processor::halt();
		}
	}

	fn open(&self, name: *const u8, flags: i32, mode: i32) -> i32 {
		let _guard = LWIP_LOCK.lock();
		let sysargs = SysOpen::new();
		proxy_write_value(&sysargs);

		let name_length = unsafe { c_strlen(name) } + 1;
		proxy_write_value(&name_length);

		let name_slice = unsafe { slice::from_raw_parts(name, name_length) };
		proxy_write_bytes(name_slice);

		proxy_write_value(&flags);
		proxy_write_value(&mode);

		let mut ret: i32 = 0;
		proxy_read_value(&mut ret);
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
		proxy_write_value(&sysargs);

		let mut ret: i32 = 0;
		proxy_read_value(&mut ret);
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
		proxy_write_value(&sysargs);

		let mut bytes_read: isize = 0;
		proxy_read_value(&mut bytes_read);

		if bytes_read > 0 {
			assert!(bytes_read as usize <= len);
			let buf_slice = unsafe { slice::from_raw_parts_mut(buf, bytes_read as usize) };
			proxy_read_bytes(buf_slice);
		}

		bytes_read
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
		proxy_write_value(&sysargs);

		let buf_slice = unsafe { slice::from_raw_parts(buf, len) };
		proxy_write_bytes(buf_slice);

		let mut bytes_written = len;
		if fd > 2 {
			proxy_read_value(&mut bytes_written);
		}

		bytes_written as isize
	}

	fn lseek(&self, fd: i32, offset: isize, whence: i32) -> isize {
		let _guard = LWIP_LOCK.lock();
		let sysargs = SysLseek::new(fd, offset, whence);
		proxy_write_value(&sysargs);

		let mut ret: isize = 0;
		proxy_read_value(&mut ret);
		ret
	}
}
