// Copyright (c) 2018 Stefan Lankes, RWTH Aachen University
//                    Colin Finck, RWTH Aachen University
//               2020 Thomas Lambertz, RWTH Aachen University
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
use core::convert::{TryFrom, TryInto};
use core::fmt::Write;
use core::{isize, ptr, slice, str};
use errno::*;
use util;

use syscalls::fs::{self, FilePerms, PosixFile, SeekWhence};

const SEEK_SET: i32 = 0;
const SEEK_CUR: i32 = 1;
const SEEK_END: i32 = 2;

impl TryFrom<i32> for SeekWhence {
	type Error = &'static str;

	fn try_from(value: i32) -> Result<Self, Self::Error> {
		match value {
			SEEK_CUR => Ok(SeekWhence::Cur),
			SEEK_SET => Ok(SeekWhence::Set),
			SEEK_END => Ok(SeekWhence::End),
			_ => Err("Got invalid seek whence parameter!"),
		}
	}
}

// TODO: these are defined in hermit-abi. Should we use a constants crate imported in both?
//const O_RDONLY: i32 = 0o0000;
const O_WRONLY: i32 = 0o0001;
const O_RDWR: i32 = 0o0002;
const O_CREAT: i32 = 0o0100;
const O_EXCL: i32 = 0o0200;
const O_TRUNC: i32 = 0o1000;
const O_APPEND: i32 = 0o2000;
const O_DIRECT: i32 = 0o40000;

fn open_flags_to_perm(flags: i32, mode: u32) -> FilePerms {
	// mode is passed in as hex (0x777). Linux/Fuse expects octal (0o777).
	// just passing mode as is to FUSE create, leads to very weird permissions: 0b0111_0111_0111 -> 'r-x rwS rwt'
	// TODO: change in stdlib
	let mode =
		match mode {
			0x777 => 0o777,
			0 => 0,
			_ => {
				info!("Mode neither 777 nor 0, should never happen with current hermit stdlib! Using 777");
				0o777
			}
		};

	let mut perms = FilePerms {
		raw: flags as u32,
		mode,
		..Default::default()
	};
	perms.write = flags & (O_WRONLY | O_RDWR) != 0;
	perms.creat = flags & (O_CREAT) != 0;
	perms.excl = flags & (O_EXCL) != 0;
	perms.trunc = flags & (O_TRUNC) != 0;
	perms.append = flags & (O_APPEND) != 0;
	perms.directio = flags & (O_DIRECT) != 0;
	if flags & !(O_WRONLY | O_RDWR | O_CREAT | O_EXCL | O_TRUNC | O_APPEND | O_DIRECT) != 0 {
		warn!("Unknown file flags used! {}", flags);
	}
	perms
}

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

	#[cfg(not(target_arch = "x86_64"))]
	fn unlink(&self, _name: *const u8) -> i32 {
		debug!("unlink is unimplemented, returning -ENOSYS");
		-ENOSYS
	}

	#[cfg(target_arch = "x86_64")]
	fn unlink(&self, name: *const u8) -> i32 {
		let name = unsafe { util::c_str_to_str(name) };
		debug!("unlink {}", name);

		fs::FILESYSTEM
			.lock()
			.unlink(&name)
			.expect("Unlinking failed!"); // TODO: error handling
		0
	}

	#[cfg(not(target_arch = "x86_64"))]
	fn open(&self, _name: *const u8, _flags: i32, _mode: i32) -> i32 {
		debug!("open is unimplemented, returning -ENOSYS");
		-ENOSYS
	}

	#[cfg(target_arch = "x86_64")]
	fn open(&self, name: *const u8, flags: i32, mode: i32) -> i32 {
		//! mode is 0x777 (0b0111_0111_0111), when flags | O_CREAT, else 0
		//! flags is bitmask of O_DEC_* defined above.
		//! (taken from rust stdlib/sys hermit target )

		let name = unsafe { util::c_str_to_str(name) };
		debug!("Open {}, {}, {}", name, flags, mode);

		let mut fs = fs::FILESYSTEM.lock();
		let fd = fs.open(&name, open_flags_to_perm(flags, mode as u32));

		if let Ok(fd) = fd {
			fd as i32
		} else {
			-1
		}
	}

	fn close(&self, fd: i32) -> i32 {
		// we don't have to close standard descriptors
		if fd < 3 {
			return 0;
		}

		let mut fs = fs::FILESYSTEM.lock();
		fs.close(fd as u64);
		return 0;
	}

	#[cfg(not(target_arch = "x86_64"))]
	fn read(&self, _fd: i32, _buf: *mut u8, _len: usize) -> isize {
		debug!("read is unimplemented, returning -ENOSYS");
		-ENOSYS as isize
	}

	#[cfg(target_arch = "x86_64")]
	fn read(&self, fd: i32, buf: *mut u8, len: usize) -> isize {
		debug!("Read! {}, {}", fd, len);

		let mut fs = fs::FILESYSTEM.lock();
		let mut read_bytes = 0;
		fs.fd_op(fd as u64, |file: &mut Box<dyn PosixFile>| {
			let dat = file.read(len as u32).unwrap(); // TODO: might fail

			read_bytes = dat.len();
			unsafe {
				core::slice::from_raw_parts_mut(buf, read_bytes).copy_from_slice(&dat);
			}
		});

		read_bytes as isize
	}

	fn write(&self, fd: i32, buf: *const u8, len: usize) -> isize {
		assert!(len <= isize::MAX as usize);

		if fd > 2 {
			// Normal file
			let buf = unsafe { slice::from_raw_parts(buf, len) };

			let mut written_bytes = 0;
			let mut fs = fs::FILESYSTEM.lock();
			fs.fd_op(fd as u64, |file: &mut Box<dyn PosixFile>| {
				written_bytes = file.write(buf).unwrap(); // TODO: might fail
			});
			debug!("Write done! {}", written_bytes);
			written_bytes as isize
		} else {
			// stdin/err/out all go to console
			unsafe {
				let slice = slice::from_raw_parts(buf, len);
				console::CONSOLE
					.lock()
					.write_str(str::from_utf8_unchecked(slice))
					.unwrap();
			}

			len as isize
		}
	}

	fn lseek(&self, fd: i32, offset: isize, whence: i32) -> isize {
		debug!("lseek! {}, {}, {}", fd, offset, whence);

		let mut fs = fs::FILESYSTEM.lock();
		let mut ret = 0;
		fs.fd_op(fd as u64, |file: &mut Box<dyn PosixFile>| {
			ret = file.lseek(offset, whence.try_into().unwrap()).unwrap(); // TODO: might fail
		});

		ret as isize
	}

	fn stat(&self, _file: *const u8, _st: usize) -> i32 {
		info!("stat is unimplemented");
		-ENOSYS
	}
}
