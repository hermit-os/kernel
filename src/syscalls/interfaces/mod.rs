// Copyright (c) 2018 Stefan Lankes, RWTH Aachen University
//                    Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

mod generic;
mod uhyve;

pub use self::generic::*;
pub use self::uhyve::*;
use alloc::boxed::Box;
use arch;
use console;
use core::fmt::Write;
use core::{isize, ptr, slice, str};
use errno::*;

#[cfg(target_arch = "x86_64")]
use arch::x86_64::kernel::fuse;

// TODO: remove
static mut already_read: bool = false;

pub trait SyscallInterface: Send + Sync {
	fn init(&self) {
		// Interface-specific initialization steps.
	}

	fn get_application_parameters(&self) -> (i32, *const *const u8, *const *const u8) {
		let argc = 1;
		let dummy = Box::new("name\0".as_ptr());
		let argv = Box::leak(dummy) as *const *const u8;
		let environ = ptr::null() as *const *const u8;

		(argc, argv, environ)
	}

	fn shutdown(&self, _arg: i32) -> ! {
		arch::processor::shutdown();
	}

	fn unlink(&self, _name: *const u8) -> i32 {
		debug!("unlink is unimplemented, returning -ENOSYS");
		-ENOSYS
	}

	#[cfg(not(target_arch = "x86_64"))]
	fn open(&self, _name: *const u8, _flags: i32, _mode: i32) -> i32 {
		debug!("open is unimplemented, returning -ENOSYS");
		-ENOSYS
	}

	#[cfg(target_arch = "x86_64")]
	fn open(&self, name: *const u8, _flags: i32, _mode: i32) -> i32 {
		info!("Open!");
		let fuse = fuse::FILESYSTEM.lock();
		let fuse = fuse.as_ref().unwrap();
		// 1.FUSE_INIT to create session
		// Already done
		// 2.FUSE_LOOKUP(FUSE_ROOT_ID, “foo”) -> nodeid
		// ugly strlen
		let namelen = unsafe {
			let mut off = name;
			while *off != 0 {
				off = off.offset(1);
			}
			off as usize - name as usize
		};
		//let namelen = 7;
		let nid = fuse.lookup(
			core::str::from_utf8(unsafe { core::slice::from_raw_parts(name, namelen) }).unwrap(),
		);
		// 3.FUSE_OPEN(nodeid, O_RDONLY) -> fh
		let fh = fuse.open(nid);
		fh as i32 // TODO: this is really bad. Works only because fh's from virtiofsd are so small!
	}

	fn close(&self, fd: i32) -> i32 {
		// we don't have to close standard descriptors
		if fd < 3 {
			return 0;
		}

		debug!("close is only implemented for stdout & stderr, returning -EINVAL");
		-EINVAL
	}

	#[cfg(not(target_arch = "x86_64"))]
	fn read(&self, _fd: i32, _buf: *mut u8, _len: usize) -> isize {
		debug!("read is unimplemented, returning -ENOSYS");
		-ENOSYS as isize
	}

	#[cfg(target_arch = "x86_64")]
	fn read(&self, fd: i32, buf: *mut u8, len: usize) -> isize {
		info!("Read!");
		// Hacky read state
		unsafe {
			if already_read {
				return 0;
			} else {
				already_read = true;
			}
		}
		let fuse = fuse::FILESYSTEM.lock();
		let fuse = fuse.as_ref().unwrap();
		// 1.FUSE_INIT to create session
		//fuse.send_hello();
		// 2.FUSE_LOOKUP(FUSE_ROOT_ID, “foo”) -> nodeid
		//let nid = fuse.lookup("testvm");
		// 3.FUSE_OPEN(nodeid, O_RDONLY) -> fh
		//let fh = fuse.open(nid);
		// 4.FUSE_READ(fh, offset, &buf, sizeof(buf)) -> nbytes
		let dat = fuse.read(fd as u64);
		let len = if len < dat.len() {
			info!("read buffer too small! {}, {}", len, dat.len());
			len
		} else {
			dat.len()
		};
		unsafe {
			core::slice::from_raw_parts_mut(buf, len).copy_from_slice(&dat[..len]);
		}
		len as isize
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
