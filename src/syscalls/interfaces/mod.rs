use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ffi::CStr;

pub use self::generic::*;
pub use self::uhyve::*;
use crate::errno::ENOENT;
use crate::syscalls::fs::{self, FileAttr};
use crate::{arch, env};

mod generic;
pub(crate) mod uhyve;

pub trait SyscallInterface: Send + Sync {
	fn init(&self) {
		// Interface-specific initialization steps.
	}

	fn get_application_parameters(&self) -> (i32, *const *const u8, *const *const u8) {
		let mut argv = Vec::new();

		let name = Box::leak(Box::new("{name}\0")).as_ptr();
		argv.push(name);

		let args = env::args();
		debug!("Setting argv as: {:?}", args);
		for arg in args {
			let ptr = Box::leak(format!("{arg}\0").into_boxed_str()).as_ptr();
			argv.push(ptr);
		}

		let mut envv = Vec::new();

		let envs = env::vars();
		debug!("Setting envv as: {:?}", envs);
		for (key, value) in envs {
			let ptr = Box::leak(format!("{key}={value}\0").into_boxed_str()).as_ptr();
			envv.push(ptr);
		}
		envv.push(core::ptr::null::<u8>());

		let argc = argv.len() as i32;
		let argv = argv.leak().as_ptr();
		// do we have more than a end marker? If not, return as null pointer
		let envv = if envv.len() == 1 {
			core::ptr::null::<*const u8>()
		} else {
			envv.leak().as_ptr()
		};

		(argc, argv, envv)
	}

	fn shutdown(&self, _arg: i32) -> ! {
		arch::processor::shutdown()
	}

	fn unlink(&self, name: *const u8) -> i32 {
		let name = unsafe { CStr::from_ptr(name as _) }.to_str().unwrap();
		debug!("unlink {}", name);

		fs::FILESYSTEM
			.lock()
			.unlink(name)
			.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |_| 0)
	}

	#[cfg(target_arch = "x86_64")]
	fn rmdir(&self, name: *const u8) -> i32 {
		let name = unsafe { CStr::from_ptr(name as _) }.to_str().unwrap();
		debug!("rmdir {}", name);

		fs::FILESYSTEM
			.lock()
			.rmdir(name)
			.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |_| 0)
	}

	#[cfg(target_arch = "x86_64")]
	fn mkdir(&self, name: *const u8, mode: u32) -> i32 {
		let name = unsafe { CStr::from_ptr(name as _) }.to_str().unwrap();
		debug!("mkdir {}, mode {}", name, mode);

		fs::FILESYSTEM
			.lock()
			.mkdir(name, mode)
			.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |_| 0)
	}

	#[cfg(not(target_arch = "x86_64"))]
	fn stat(&self, _name: *const u8, _stat: *mut FileAttr) -> i32 {
		debug!("stat is unimplemented, returning -ENOSYS");
		-ENOSYS
	}

	#[cfg(target_arch = "x86_64")]
	fn stat(&self, name: *const u8, stat: *mut FileAttr) -> i32 {
		let name = unsafe { CStr::from_ptr(name as _) }.to_str().unwrap();
		debug!("stat {}", name);

		fs::FILESYSTEM
			.lock()
			.stat(name, stat)
			.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |_| 0)
	}

	#[cfg(not(target_arch = "x86_64"))]
	fn lstat(&self, _name: *const u8, _stat: *mut FileAttr) -> i32 {
		debug!("lstat is unimplemented, returning -ENOSYS");
		-ENOSYS
	}

	#[cfg(target_arch = "x86_64")]
	fn lstat(&self, name: *const u8, stat: *mut FileAttr) -> i32 {
		let name = unsafe { CStr::from_ptr(name as _) }.to_str().unwrap();
		debug!("lstat {}", name);

		fs::FILESYSTEM
			.lock()
			.lstat(name, stat)
			.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |_| 0)
	}
}
