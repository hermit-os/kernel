#![allow(clippy::result_unit_err)]

#[cfg(feature = "newlib")]
use hermit_sync::InterruptTicketMutex;
use hermit_sync::Lazy;

pub use self::condvar::*;
pub use self::entropy::*;
pub use self::futex::*;
pub use self::processor::*;
pub use self::recmutex::*;
pub use self::semaphore::*;
pub use self::spinlock::*;
pub use self::system::*;
pub use self::tasks::*;
pub use self::timer::*;
use crate::env;
use crate::fd::{dup_object, get_object, remove_object, FileDescriptor};
use crate::syscalls::interfaces::SyscallInterface;
#[cfg(target_os = "none")]
use crate::{__sys_free, __sys_malloc, __sys_realloc};

mod condvar;
mod entropy;
pub(crate) mod fs;
mod futex;
mod interfaces;
#[cfg(feature = "newlib")]
mod lwip;
#[cfg(all(feature = "tcp", not(feature = "newlib")))]
pub mod net;
mod processor;
mod recmutex;
mod semaphore;
mod spinlock;
mod system;
mod tasks;
mod timer;

#[cfg(feature = "newlib")]
const LWIP_FD_BIT: i32 = 1 << 30;

#[cfg(feature = "newlib")]
pub static LWIP_LOCK: InterruptTicketMutex<()> = InterruptTicketMutex::new(());

pub(crate) static SYS: Lazy<&'static dyn SyscallInterface> = Lazy::new(|| {
	if env::is_uhyve() {
		&self::interfaces::Uhyve
	} else {
		&self::interfaces::Generic
	}
});

/// Shuts down the machine.
///
/// This does not require the syscall interface to be initialized.
pub(crate) fn shutdown(arg: i32) -> ! {
	if env::is_uhyve() {
		crate::syscalls::interfaces::Uhyve.shutdown(arg)
	} else {
		crate::syscalls::interfaces::Generic.shutdown(arg)
	}
}

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
	__sys_malloc(size, align)
}

#[cfg(target_os = "none")]
#[no_mangle]
pub extern "C" fn sys_realloc(ptr: *mut u8, size: usize, align: usize, new_size: usize) -> *mut u8 {
	__sys_realloc(ptr, size, align, new_size)
}

#[cfg(target_os = "none")]
#[no_mangle]
pub extern "C" fn sys_free(ptr: *mut u8, size: usize, align: usize) {
	__sys_free(ptr, size, align)
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
	SYS.unlink(name)
}

#[no_mangle]
pub extern "C" fn sys_unlink(name: *const u8) -> i32 {
	kernel_function!(__sys_unlink(name))
}

extern "C" fn __sys_open(name: *const u8, flags: i32, mode: i32) -> FileDescriptor {
	crate::fd::open(name, flags, mode).map_or_else(|e| e, |v| v)
}

#[no_mangle]
pub extern "C" fn sys_open(name: *const u8, flags: i32, mode: i32) -> FileDescriptor {
	kernel_function!(__sys_open(name, flags, mode))
}

extern "C" fn __sys_close(fd: FileDescriptor) -> i32 {
	let obj = remove_object(fd);
	obj.map_or_else(|e| e, |_| 0)
}

#[no_mangle]
pub extern "C" fn sys_close(fd: FileDescriptor) -> i32 {
	kernel_function!(__sys_close(fd))
}

extern "C" fn __sys_read(fd: FileDescriptor, buf: *mut u8, len: usize) -> isize {
	let obj = get_object(fd);
	obj.map_or_else(|e| e as isize, |v| (*v).read(buf, len))
}

#[no_mangle]
pub extern "C" fn sys_read(fd: FileDescriptor, buf: *mut u8, len: usize) -> isize {
	kernel_function!(__sys_read(fd, buf, len))
}

extern "C" fn __sys_write(fd: FileDescriptor, buf: *const u8, len: usize) -> isize {
	let obj = get_object(fd);
	obj.map_or_else(|e| e as isize, |v| (*v).write(buf, len))
}

#[no_mangle]
pub extern "C" fn sys_write(fd: FileDescriptor, buf: *const u8, len: usize) -> isize {
	kernel_function!(__sys_write(fd, buf, len))
}

extern "C" fn __sys_ioctl(fd: FileDescriptor, cmd: i32, argp: *mut core::ffi::c_void) -> i32 {
	let obj = get_object(fd);
	obj.map_or_else(|e| e, |v| (*v).ioctl(cmd, argp))
}

#[no_mangle]
pub extern "C" fn sys_ioctl(fd: FileDescriptor, cmd: i32, argp: *mut core::ffi::c_void) -> i32 {
	kernel_function!(__sys_ioctl(fd, cmd, argp))
}

extern "C" fn __sys_lseek(fd: FileDescriptor, offset: isize, whence: i32) -> isize {
	let obj = get_object(fd);
	obj.map_or_else(|e| e as isize, |v| (*v).lseek(offset, whence))
}

#[no_mangle]
pub extern "C" fn sys_lseek(fd: FileDescriptor, offset: isize, whence: i32) -> isize {
	kernel_function!(__sys_lseek(fd, offset, whence))
}

extern "C" fn __sys_stat(file: *const u8, st: usize) -> i32 {
	SYS.stat(file, st)
}

#[no_mangle]
pub extern "C" fn sys_stat(file: *const u8, st: usize) -> i32 {
	kernel_function!(__sys_stat(file, st))
}

extern "C" fn __sys_dup(fd: i32) -> i32 {
	dup_object(fd).map_or_else(|e| e, |v| v)
}

#[no_mangle]
pub extern "C" fn sys_dup(fd: i32) -> i32 {
	kernel_function!(__sys_dup(fd))
}
