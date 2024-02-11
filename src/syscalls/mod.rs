#![allow(clippy::result_unit_err)]

use core::ffi::CStr;
use core::marker::PhantomData;

#[cfg(feature = "newlib")]
use hermit_sync::InterruptTicketMutex;
use hermit_sync::Lazy;

pub use self::condvar::*;
pub use self::entropy::*;
pub use self::futex::*;
pub use self::processor::*;
#[cfg(feature = "newlib")]
pub use self::recmutex::*;
pub use self::semaphore::*;
pub use self::spinlock::*;
pub use self::system::*;
pub use self::tasks::*;
pub use self::timer::*;
use crate::env;
use crate::fd::{
	dup_object, get_object, remove_object, AccessPermission, EventFlags, FileDescriptor, IoCtl,
	IoError, OpenOption, PollFd,
};
use crate::fs::{self, FileAttr};
use crate::syscalls::interfaces::SyscallInterface;
#[cfg(target_os = "none")]
use crate::{__sys_free, __sys_malloc, __sys_realloc};

mod condvar;
mod entropy;
mod futex;
mod interfaces;
#[cfg(feature = "newlib")]
mod lwip;
#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
pub mod net;
mod processor;
#[cfg(feature = "newlib")]
mod recmutex;
mod semaphore;
mod spinlock;
mod system;
mod tasks;
mod timer;

#[cfg(feature = "newlib")]
const LWIP_FD_BIT: i32 = 1 << 30;

#[cfg(feature = "newlib")]
pub(crate) static LWIP_LOCK: InterruptTicketMutex<()> = InterruptTicketMutex::new(());

pub(crate) static SYS: Lazy<&'static dyn SyscallInterface> = Lazy::new(|| {
	if env::is_uhyve() {
		&self::interfaces::Uhyve
	} else {
		&self::interfaces::Generic
	}
});

pub(crate) fn init() {
	Lazy::force(&SYS);

	// Perform interface-specific initialization steps.
	SYS.init();

	init_entropy();
	#[cfg(feature = "newlib")]
	sbrk_init();
}

#[cfg(target_os = "none")]
#[no_mangle]
pub extern "C" fn sys_malloc(size: usize, align: usize) -> *mut u8 {
	kernel_function!(__sys_malloc(size, align))
}

#[cfg(target_os = "none")]
#[no_mangle]
pub extern "C" fn sys_realloc(ptr: *mut u8, size: usize, align: usize, new_size: usize) -> *mut u8 {
	kernel_function!(__sys_realloc(ptr, size, align, new_size))
}

#[cfg(target_os = "none")]
#[no_mangle]
pub extern "C" fn sys_free(ptr: *mut u8, size: usize, align: usize) {
	kernel_function!(__sys_free(ptr, size, align))
}

pub(crate) fn get_application_parameters() -> (i32, *const *const u8, *const *const u8) {
	SYS.get_application_parameters()
}

pub(crate) extern "C" fn __sys_shutdown(arg: i32) -> ! {
	// print some performance statistics
	crate::arch::kernel::print_statistics();

	SYS.shutdown(arg)
}

#[no_mangle]
pub extern "C" fn sys_shutdown(arg: i32) -> ! {
	kernel_function!(__sys_shutdown(arg))
}

extern "C" fn __sys_unlink(name: *const u8) -> i32 {
	let name = unsafe { CStr::from_ptr(name as _) }.to_str().unwrap();

	fs::FILESYSTEM
		.get()
		.unwrap()
		.unlink(name)
		.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |_| 0)
}

#[no_mangle]
pub extern "C" fn sys_unlink(name: *const u8) -> i32 {
	kernel_function!(__sys_unlink(name))
}

extern "C" fn __sys_mkdir(name: *const u8, mode: u32) -> i32 {
	let name = unsafe { CStr::from_ptr(name as _) }.to_str().unwrap();
	let mode = if let Some(mode) = AccessPermission::from_bits(mode) {
		mode
	} else {
		return -crate::errno::EINVAL;
	};

	fs::FILESYSTEM
		.get()
		.unwrap()
		.mkdir(name, mode)
		.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |_| 0)
}

#[no_mangle]
pub extern "C" fn sys_mkdir(name: *const u8, mode: u32) -> i32 {
	kernel_function!(__sys_mkdir(name, mode))
}

extern "C" fn __sys_rmdir(name: *const u8) -> i32 {
	let name = unsafe { CStr::from_ptr(name as _) }.to_str().unwrap();

	fs::FILESYSTEM
		.get()
		.unwrap()
		.rmdir(name)
		.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |_| 0)
}

#[no_mangle]
pub extern "C" fn sys_rmdir(name: *const u8) -> i32 {
	kernel_function!(__sys_rmdir(name))
}

extern "C" fn __sys_stat(name: *const u8, stat: *mut FileAttr) -> i32 {
	let name = unsafe { CStr::from_ptr(name as _) }.to_str().unwrap();

	match fs::FILESYSTEM.get().unwrap().stat(name) {
		Ok(attr) => unsafe {
			*stat = attr;
			0
		},
		Err(e) => -num::ToPrimitive::to_i32(&e).unwrap(),
	}
}

#[no_mangle]
pub extern "C" fn sys_stat(name: *const u8, stat: *mut FileAttr) -> i32 {
	kernel_function!(__sys_stat(name, stat))
}

extern "C" fn __sys_lstat(name: *const u8, stat: *mut FileAttr) -> i32 {
	let name = unsafe { CStr::from_ptr(name as _) }.to_str().unwrap();

	match fs::FILESYSTEM.get().unwrap().lstat(name) {
		Ok(attr) => unsafe {
			*stat = attr;
			0
		},
		Err(e) => -num::ToPrimitive::to_i32(&e).unwrap(),
	}
}

#[no_mangle]
pub extern "C" fn sys_lstat(name: *const u8, stat: *mut FileAttr) -> i32 {
	kernel_function!(__sys_lstat(name, stat))
}

extern "C" fn __sys_fstat(fd: FileDescriptor, stat: *mut FileAttr) -> i32 {
	let stat = unsafe { &mut *stat };
	let obj = get_object(fd);
	obj.map_or_else(
		|e| -num::ToPrimitive::to_i32(&e).unwrap(),
		|v| {
			(*v).fstat(stat)
				.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |_| 0)
		},
	)
}

#[no_mangle]
pub extern "C" fn sys_fstat(fd: FileDescriptor, stat: *mut FileAttr) -> i32 {
	kernel_function!(__sys_fstat(fd, stat))
}

extern "C" fn __sys_opendir(name: *const u8) -> FileDescriptor {
	if let Ok(name) = unsafe { CStr::from_ptr(name as _) }.to_str() {
		crate::fs::opendir(name).map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |v| v)
	} else {
		-crate::errno::EINVAL
	}
}

#[no_mangle]
pub extern "C" fn sys_opendir(name: *const u8) -> FileDescriptor {
	kernel_function!(__sys_opendir(name))
}

extern "C" fn __sys_open(name: *const u8, flags: i32, mode: u32) -> FileDescriptor {
	let flags = if let Some(flags) = OpenOption::from_bits(flags) {
		flags
	} else {
		return -crate::errno::EINVAL;
	};
	let mode = if let Some(mode) = AccessPermission::from_bits(mode) {
		mode
	} else {
		return -crate::errno::EINVAL;
	};

	if let Ok(name) = unsafe { CStr::from_ptr(name as _) }.to_str() {
		crate::fd::open(name, flags, mode)
			.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |v| v)
	} else {
		-crate::errno::EINVAL
	}
}

#[no_mangle]
pub extern "C" fn sys_open(name: *const u8, flags: i32, mode: u32) -> FileDescriptor {
	kernel_function!(__sys_open(name, flags, mode))
}

extern "C" fn __sys_close(fd: FileDescriptor) -> i32 {
	let obj = remove_object(fd);
	obj.map_or_else(
		|e| -num::ToPrimitive::to_i32(&e).unwrap(),
		|v| {
			v.close();
			0
		},
	)
}

#[no_mangle]
pub extern "C" fn sys_close(fd: FileDescriptor) -> i32 {
	kernel_function!(__sys_close(fd))
}

extern "C" fn __sys_read(fd: FileDescriptor, buf: *mut u8, len: usize) -> isize {
	let slice = unsafe { core::slice::from_raw_parts_mut(buf, len) };
	crate::fd::read(fd, slice).map_or_else(
		|e| -num::ToPrimitive::to_isize(&e).unwrap(),
		|v| v.try_into().unwrap(),
	)
}

#[no_mangle]
pub extern "C" fn sys_read(fd: FileDescriptor, buf: *mut u8, len: usize) -> isize {
	kernel_function!(__sys_read(fd, buf, len))
}

extern "C" fn __sys_write(fd: FileDescriptor, buf: *const u8, len: usize) -> isize {
	let slice = unsafe { core::slice::from_raw_parts(buf, len) };
	crate::fd::write(fd, slice).map_or_else(
		|e| -num::ToPrimitive::to_isize(&e).unwrap(),
		|v| v.try_into().unwrap(),
	)
}

#[no_mangle]
pub extern "C" fn sys_write(fd: FileDescriptor, buf: *const u8, len: usize) -> isize {
	kernel_function!(__sys_write(fd, buf, len))
}

extern "C" fn __sys_ioctl(fd: FileDescriptor, cmd: i32, argp: *mut core::ffi::c_void) -> i32 {
	const FIONBIO: i32 = 0x8008667eu32 as i32;

	if cmd == FIONBIO {
		let value = unsafe { *(argp as *const i32) };

		let obj = get_object(fd);
		obj.map_or_else(
			|e| -num::ToPrimitive::to_i32(&e).unwrap(),
			|v| {
				(*v).ioctl(IoCtl::NonBlocking, value != 0)
					.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |_| 0)
			},
		)
	} else {
		-crate::errno::EINVAL
	}
}

#[no_mangle]
pub extern "C" fn sys_ioctl(fd: FileDescriptor, cmd: i32, argp: *mut core::ffi::c_void) -> i32 {
	kernel_function!(__sys_ioctl(fd, cmd, argp))
}

extern "C" fn __sys_fcntl(fd: i32, cmd: i32, arg: i32) -> i32 {
	const F_SETFD: i32 = 2;
	const F_SETFL: i32 = 4;
	const FD_CLOEXEC: i32 = 1;
	const O_NONBLOCK: i32 = 0o4000;

	if cmd == F_SETFD && arg == FD_CLOEXEC {
		0
	} else if cmd == F_SETFL && arg == O_NONBLOCK {
		let obj = get_object(fd);
		obj.map_or_else(
			|e| -num::ToPrimitive::to_i32(&e).unwrap(),
			|v| {
				(*v).ioctl(IoCtl::NonBlocking, true)
					.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |_| 0)
			},
		)
	} else {
		-crate::errno::EINVAL
	}
}

/// manipulate file descriptor
#[no_mangle]
pub extern "C" fn sys_fcntl(fd: i32, cmd: i32, arg: i32) -> i32 {
	kernel_function!(__sys_fcntl(fd, cmd, arg))
}

extern "C" fn __sys_lseek(fd: FileDescriptor, offset: isize, whence: i32) -> isize {
	let obj = get_object(fd);
	obj.map_or_else(
		|e| -num::ToPrimitive::to_isize(&e).unwrap(),
		|v| {
			(*v).lseek(offset, num::FromPrimitive::from_i32(whence).unwrap())
				.map_or_else(|e| -num::ToPrimitive::to_isize(&e).unwrap(), |_| 0)
		},
	)
}

#[no_mangle]
pub extern "C" fn sys_lseek(fd: FileDescriptor, offset: isize, whence: i32) -> isize {
	kernel_function!(__sys_lseek(fd, offset, whence))
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Dirent64 {
	/// 64-bit inode number
	pub d_ino: u64,
	/// 64-bit offset to next structure
	pub d_off: i64,
	/// Size of this dirent
	pub d_reclen: u16,
	/// File type
	pub d_type: u8,
	/// Filename (null-terminated)
	pub d_name: PhantomData<u8>,
}

extern "C" fn __sys_getdents64(fd: FileDescriptor, dirp: *mut Dirent64, count: usize) -> i64 {
	if dirp.is_null() || count == 0 {
		return -crate::errno::EINVAL as i64;
	}

	let limit = dirp as usize + count;
	let mut dirp: *mut Dirent64 = dirp;
	let mut offset: i64 = 0;
	let obj = get_object(fd);
	obj.map_or_else(
		|_| -crate::errno::EINVAL as i64,
		|v| {
			(*v).readdir().map_or_else(
				|e| -num::ToPrimitive::to_i64(&e).unwrap(),
				|v| {
					for i in v.iter() {
						let len = i.name.len();
						if dirp as usize + core::mem::size_of::<Dirent64>() + len + 1 >= limit {
							return -crate::errno::EINVAL as i64;
						}

						let dir = unsafe { &mut *dirp };

						dir.d_ino = 0;
						dir.d_type = 0;
						dir.d_reclen = (core::mem::size_of::<Dirent64>() + len + 1)
							.try_into()
							.unwrap();
						offset += i64::from(dir.d_reclen);
						dir.d_off = offset;

						// copy null-terminated filename
						let s = &mut dir.d_name as *mut _ as *mut u8;
						unsafe {
							core::ptr::copy_nonoverlapping(i.name.as_ptr(), s, len);
							s.add(len).write_bytes(0, 1);
						}

						dirp = unsafe {
							(dirp as *mut u8).add(dir.d_reclen as usize) as *mut Dirent64
						};
					}

					offset
				},
			)
		},
	)
}

#[no_mangle]
pub extern "C" fn sys_getdents64(fd: FileDescriptor, dirp: *mut Dirent64, count: usize) -> i64 {
	kernel_function!(__sys_getdents64(fd, dirp, count))
}

extern "C" fn __sys_dup(fd: i32) -> i32 {
	dup_object(fd).map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |v| v)
}

#[no_mangle]
pub extern "C" fn sys_dup(fd: i32) -> i32 {
	kernel_function!(__sys_dup(fd))
}

extern "C" fn __sys_poll(fds: *mut PollFd, nfds: usize, timeout: i32) -> i32 {
	let slice = unsafe { core::slice::from_raw_parts_mut(fds, nfds) };
	let timeout = if timeout >= 0 {
		Some(core::time::Duration::from_millis(
			timeout.try_into().unwrap(),
		))
	} else {
		None
	};

	crate::fd::poll(slice, timeout).map_or_else(
		|e| {
			if e == IoError::ETIME {
				0
			} else {
				-num::ToPrimitive::to_i32(&e).unwrap()
			}
		},
		|v| v.try_into().unwrap(),
	)
}

#[no_mangle]
pub extern "C" fn sys_poll(fds: *mut PollFd, nfds: usize, timeout: i32) -> i32 {
	kernel_function!(__sys_poll(fds, nfds, timeout))
}

extern "C" fn __sys_eventfd(initval: u64, flags: i16) -> i32 {
	if let Some(flags) = EventFlags::from_bits(flags) {
		crate::fd::eventfd(initval, flags)
			.map_or_else(|e| -num::ToPrimitive::to_i32(&e).unwrap(), |v| v)
	} else {
		-crate::errno::EINVAL
	}
}

#[no_mangle]
pub extern "C" fn sys_eventfd(initval: u64, flags: i16) -> i32 {
	kernel_function!(__sys_eventfd(initval, flags))
}

extern "C" fn __sys_image_start_addr() -> usize {
	crate::mm::kernel_start_address().0.try_into().unwrap()
}

#[no_mangle]
pub extern "C" fn sys_image_start_addr() -> usize {
	kernel_function!(__sys_image_start_addr())
}
