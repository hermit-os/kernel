#![allow(clippy::result_unit_err)]

#[cfg(all(target_os = "none", not(feature = "common-os")))]
use core::alloc::{GlobalAlloc, Layout};
use core::ffi::{CStr, c_char};
use core::marker::PhantomData;
use core::ptr;

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
use crate::executor::block_on;
use crate::fd::{
	self, AccessPermission, EventFlags, FileDescriptor, OpenOption, PollFd, dup_object,
	dup_object2, get_object, isatty, remove_object,
};
use crate::fs::{self, FileAttr, SeekWhence};
#[cfg(all(target_os = "none", not(feature = "common-os")))]
use crate::mm::ALLOCATOR;
use crate::syscalls::interfaces::SyscallInterface;
use crate::{env, io};

mod condvar;
mod entropy;
mod futex;
pub(crate) mod interfaces;
#[cfg(feature = "mmap")]
mod mmap;
mod processor;
#[cfg(feature = "newlib")]
mod recmutex;
mod semaphore;
#[cfg(any(feature = "tcp", feature = "udp", feature = "vsock"))]
pub mod socket;
mod spinlock;
mod system;
#[cfg(feature = "common-os")]
pub(crate) mod table;
mod tasks;
mod timer;

pub(crate) static SYS: Lazy<&'static dyn SyscallInterface> = Lazy::new(|| {
	if env::is_uhyve() {
		&self::interfaces::Uhyve
	} else {
		&self::interfaces::Generic
	}
});

#[repr(C)]
#[derive(Debug, Clone, Copy)]
/// Describes  a  region  of  memory, beginning at `iov_base` address and with the size of `iov_len` bytes.
struct iovec {
	/// Starting address
	pub iov_base: *mut u8,
	/// Size of the memory pointed to by iov_base.
	pub iov_len: usize,
}

const IOV_MAX: usize = 1024;

pub(crate) fn init() {
	Lazy::force(&SYS);

	// Perform interface-specific initialization steps.
	SYS.init();

	init_entropy();
}

/// Interface to allocate memory from system heap
///
/// # Errors
/// Returning a null pointer indicates that either memory is exhausted or
/// `size` and `align` do not meet this allocator's size or alignment constraints.
///
#[cfg(all(target_os = "none", not(feature = "common-os")))]
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_alloc(size: usize, align: usize) -> *mut u8 {
	let layout_res = Layout::from_size_align(size, align);
	if layout_res.is_err() || size == 0 {
		warn!("__sys_alloc called with size {size:#x}, align {align:#x} is an invalid layout!");
		return core::ptr::null_mut();
	}
	let layout = layout_res.unwrap();
	let ptr = unsafe { ALLOCATOR.alloc(layout) };

	trace!("__sys_alloc: allocate memory at {ptr:p} (size {size:#x}, align {align:#x})");

	ptr
}

#[cfg(all(target_os = "none", not(feature = "common-os")))]
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_alloc_zeroed(size: usize, align: usize) -> *mut u8 {
	let layout_res = Layout::from_size_align(size, align);
	if layout_res.is_err() || size == 0 {
		warn!(
			"__sys_alloc_zeroed called with size {size:#x}, align {align:#x} is an invalid layout!"
		);
		return core::ptr::null_mut();
	}
	let layout = layout_res.unwrap();
	let ptr = unsafe { ALLOCATOR.alloc_zeroed(layout) };

	trace!("__sys_alloc_zeroed: allocate memory at {ptr:p} (size {size:#x}, align {align:#x})");

	ptr
}

#[cfg(all(target_os = "none", not(feature = "common-os")))]
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_malloc(size: usize, align: usize) -> *mut u8 {
	let layout_res = Layout::from_size_align(size, align);
	if layout_res.is_err() || size == 0 {
		warn!("__sys_malloc called with size {size:#x}, align {align:#x} is an invalid layout!");
		return core::ptr::null_mut();
	}
	let layout = layout_res.unwrap();
	let ptr = unsafe { ALLOCATOR.alloc(layout) };

	trace!("__sys_malloc: allocate memory at {ptr:p} (size {size:#x}, align {align:#x})");

	ptr
}

/// Shrink or grow a block of memory to the given `new_size`. The block is described by the given
/// ptr pointer and layout. If this returns a non-null pointer, then ownership of the memory block
/// referenced by ptr has been transferred to this allocator. The memory may or may not have been
/// deallocated, and should be considered unusable (unless of course it was transferred back to the
/// caller again via the return value of this method). The new memory block is allocated with
/// layout, but with the size updated to new_size.
/// If this method returns null, then ownership of the memory block has not been transferred to this
/// allocator, and the contents of the memory block are unaltered.
///
/// # Safety
/// This function is unsafe because undefined behavior can result if the caller does not ensure all
/// of the following:
/// - `ptr` must be currently allocated via this allocator,
/// - `size` and `align` must be the same layout that was used to allocate that block of memory.
/// ToDO: verify if the same values for size and align always lead to the same layout
///
/// # Errors
/// Returns null if the new layout does not meet the size and alignment constraints of the
/// allocator, or if reallocation otherwise fails.
#[cfg(all(target_os = "none", not(feature = "common-os")))]
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_realloc(
	ptr: *mut u8,
	size: usize,
	align: usize,
	new_size: usize,
) -> *mut u8 {
	unsafe {
		let layout_res = Layout::from_size_align(size, align);
		if layout_res.is_err() || size == 0 || new_size == 0 {
			warn!(
				"__sys_realloc called with ptr {ptr:p}, size {size:#x}, align {align:#x}, new_size {new_size:#x} is an invalid layout!"
			);
			return core::ptr::null_mut();
		}
		let layout = layout_res.unwrap();
		let new_ptr = ALLOCATOR.realloc(ptr, layout, new_size);

		if new_ptr.is_null() {
			debug!(
				"__sys_realloc failed to resize ptr {ptr:p} with size {size:#x}, align {align:#x}, new_size {new_size:#x} !"
			);
		} else {
			trace!("__sys_realloc: resized memory at {ptr:p}, new address {new_ptr:p}");
		}
		new_ptr
	}
}

/// Interface to deallocate a memory region from the system heap
///
/// # Safety
/// This function is unsafe because undefined behavior can result if the caller does not ensure all of the following:
/// - ptr must denote a block of memory currently allocated via this allocator,
/// - `size` and `align` must be the same values that were used to allocate that block of memory
/// ToDO: verify if the same values for size and align always lead to the same layout
///
/// # Errors
/// May panic if debug assertions are enabled and invalid parameters `size` or `align` where passed.
#[cfg(all(target_os = "none", not(feature = "common-os")))]
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_dealloc(ptr: *mut u8, size: usize, align: usize) {
	unsafe {
		let layout_res = Layout::from_size_align(size, align);
		if layout_res.is_err() || size == 0 {
			warn!(
				"__sys_dealloc called with size {size:#x}, align {align:#x} is an invalid layout!"
			);
			debug_assert!(layout_res.is_err(), "__sys_dealloc error: Invalid layout");
			debug_assert_ne!(size, 0, "__sys_dealloc error: size cannot be 0");
		} else {
			trace!("sys_free: deallocate memory at {ptr:p} (size {size:#x})");
		}
		let layout = layout_res.unwrap();
		ALLOCATOR.dealloc(ptr, layout);
	}
}

#[cfg(all(target_os = "none", not(feature = "common-os")))]
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_free(ptr: *mut u8, size: usize, align: usize) {
	unsafe {
		let layout_res = Layout::from_size_align(size, align);
		if layout_res.is_err() || size == 0 {
			warn!("__sys_free called with size {size:#x}, align {align:#x} is an invalid layout!");
			debug_assert!(layout_res.is_err(), "__sys_free error: Invalid layout");
			debug_assert_ne!(size, 0, "__sys_free error: size cannot be 0");
		} else {
			trace!("sys_free: deallocate memory at {ptr:p} (size {size:#x})");
		}
		let layout = layout_res.unwrap();
		ALLOCATOR.dealloc(ptr, layout);
	}
}

pub(crate) fn get_application_parameters() -> (i32, *const *const u8, *const *const u8) {
	SYS.get_application_parameters()
}

pub(crate) fn shutdown(arg: i32) -> ! {
	// print some performance statistics
	crate::arch::kernel::print_statistics();

	SYS.shutdown(arg)
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_unlink(name: *const c_char) -> i32 {
	let name = unsafe { CStr::from_ptr(name) }.to_str().unwrap();

	fs::unlink(name).map_or_else(|e| -i32::from(e), |()| 0)
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_mkdir(name: *const c_char, mode: u32) -> i32 {
	let name = unsafe { CStr::from_ptr(name) }.to_str().unwrap();
	let Some(mode) = AccessPermission::from_bits(mode) else {
		return -crate::errno::EINVAL;
	};

	crate::fs::create_dir(name, mode).map_or_else(|e| -i32::from(e), |()| 0)
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_rmdir(name: *const c_char) -> i32 {
	let name = unsafe { CStr::from_ptr(name) }.to_str().unwrap();

	crate::fs::remove_dir(name).map_or_else(|e| -i32::from(e), |()| 0)
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_stat(name: *const c_char, stat: *mut FileAttr) -> i32 {
	let name = unsafe { CStr::from_ptr(name) }.to_str().unwrap();

	match fs::read_stat(name) {
		Ok(attr) => unsafe {
			*stat = attr;
			0
		},
		Err(e) => -i32::from(e),
	}
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_lstat(name: *const c_char, stat: *mut FileAttr) -> i32 {
	let name = unsafe { CStr::from_ptr(name) }.to_str().unwrap();

	match fs::read_lstat(name) {
		Ok(attr) => unsafe {
			*stat = attr;
			0
		},
		Err(e) => -i32::from(e),
	}
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_fstat(fd: FileDescriptor, stat: *mut FileAttr) -> i32 {
	if stat.is_null() {
		return -crate::errno::EINVAL;
	}

	crate::fd::fstat(fd).map_or_else(
		|e| -i32::from(e),
		|v| unsafe {
			*stat = v;
			0
		},
	)
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_opendir(name: *const c_char) -> FileDescriptor {
	if let Ok(name) = unsafe { CStr::from_ptr(name) }.to_str() {
		crate::fs::opendir(name).unwrap_or_else(|e| -i32::from(e))
	} else {
		-crate::errno::EINVAL
	}
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_open(name: *const c_char, flags: i32, mode: u32) -> FileDescriptor {
	let Some(flags) = OpenOption::from_bits(flags) else {
		return -crate::errno::EINVAL;
	};
	let Some(mode) = AccessPermission::from_bits(mode) else {
		return -crate::errno::EINVAL;
	};

	if let Ok(name) = unsafe { CStr::from_ptr(name) }.to_str() {
		crate::fs::open(name, flags, mode).unwrap_or_else(|e| -i32::from(e))
	} else {
		-crate::errno::EINVAL
	}
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub extern "C" fn sys_close(fd: FileDescriptor) -> i32 {
	let obj = remove_object(fd);
	obj.map_or_else(|e| -i32::from(e), |_| 0)
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_read(fd: FileDescriptor, buf: *mut u8, len: usize) -> isize {
	let slice = unsafe { core::slice::from_raw_parts_mut(buf.cast(), len) };
	crate::fd::read(fd, slice).map_or_else(
		|e| isize::try_from(-i32::from(e)).unwrap(),
		|v| v.try_into().unwrap(),
	)
}

/// `read()` attempts to read `nbyte` of data to the object referenced by the
/// descriptor `fd` from a buffer. `read()` performs the same
/// action, but scatters the input data from the `iovcnt` buffers specified by the
/// members of the iov array: `iov[0], iov[1], ..., iov[iovcnt-1]`.
///
/// ```
/// struct iovec {
///     char   *iov_base;  /* Base address. */
///     size_t iov_len;    /* Length. */
/// };
/// ```
///
/// Each `iovec` entry specifies the base address and length of an area in memory from
/// which data should be written.  `readv()` will always fill an completely
/// before proceeding to the next.
#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_readv(fd: i32, iov: *const iovec, iovcnt: usize) -> isize {
	if !(0..=IOV_MAX).contains(&iovcnt) {
		return (-crate::errno::EINVAL).try_into().unwrap();
	}

	let mut read_bytes: isize = 0;
	let iovec_buffers = unsafe { core::slice::from_raw_parts(iov, iovcnt) };

	for iovec_buf in iovec_buffers {
		let buf = unsafe {
			core::slice::from_raw_parts_mut(iovec_buf.iov_base.cast(), iovec_buf.iov_len)
		};

		let len = crate::fd::read(fd, buf).map_or_else(
			|e| isize::try_from(-i32::from(e)).unwrap(),
			|v| v.try_into().unwrap(),
		);

		if len < 0 {
			return len;
		}

		read_bytes += len;

		if len < isize::try_from(iovec_buf.iov_len).unwrap() {
			return read_bytes;
		}
	}

	read_bytes
}

unsafe fn write(fd: FileDescriptor, buf: *const u8, len: usize) -> isize {
	let slice = unsafe { core::slice::from_raw_parts(buf, len) };
	crate::fd::write(fd, slice).map_or_else(
		|e| isize::try_from(-i32::from(e)).unwrap(),
		|v| v.try_into().unwrap(),
	)
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_write(fd: FileDescriptor, buf: *const u8, len: usize) -> isize {
	unsafe { write(fd, buf, len) }
}

/// `write()` attempts to write `nbyte` of data to the object referenced by the
/// descriptor `fd` from a buffer. `writev()` performs the same
/// action, but gathers the output data from the `iovcnt` buffers specified by the
/// members of the iov array: `iov[0], iov[1], ..., iov[iovcnt-1]`.
///
/// ```
/// struct iovec {
///     char   *iov_base;  /* Base address. */
///     size_t iov_len;    /* Length. */
/// };
/// ```
///
/// Each `iovec` entry specifies the base address and length of an area in memory from
/// which data should be written.  `writev()` will always write a
/// complete area before proceeding to the next.
#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_writev(fd: FileDescriptor, iov: *const iovec, iovcnt: usize) -> isize {
	if !(0..=IOV_MAX).contains(&iovcnt) {
		return (-crate::errno::EINVAL).try_into().unwrap();
	}

	let mut written_bytes: isize = 0;
	let iovec_buffers = unsafe { core::slice::from_raw_parts(iov, iovcnt) };

	for iovec_buf in iovec_buffers {
		let buf = unsafe { core::slice::from_raw_parts(iovec_buf.iov_base, iovec_buf.iov_len) };

		let len = crate::fd::write(fd, buf).map_or_else(
			|e| isize::try_from(-i32::from(e)).unwrap(),
			|v| v.try_into().unwrap(),
		);

		if len < 0 {
			return len;
		}

		written_bytes += len;

		if len < isize::try_from(iovec_buf.iov_len).unwrap() {
			return written_bytes;
		}
	}

	written_bytes
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_ioctl(
	fd: FileDescriptor,
	cmd: i32,
	argp: *mut core::ffi::c_void,
) -> i32 {
	const FIONBIO: i32 = 0x8008_667eu32 as i32;

	if cmd == FIONBIO {
		let value = unsafe { *(argp as *const i32) };
		let status_flags = if value != 0 {
			fd::StatusFlags::O_NONBLOCK
		} else {
			fd::StatusFlags::empty()
		};

		let obj = get_object(fd);
		obj.map_or_else(
			|e| -i32::from(e),
			|v| {
				block_on((*v).set_status_flags(status_flags), None)
					.map_or_else(|e| -i32::from(e), |()| 0)
			},
		)
	} else {
		-crate::errno::EINVAL
	}
}

/// manipulate file descriptor
#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub extern "C" fn sys_fcntl(fd: i32, cmd: i32, arg: i32) -> i32 {
	const F_SETFD: i32 = 2;
	const F_GETFL: i32 = 3;
	const F_SETFL: i32 = 4;
	const FD_CLOEXEC: i32 = 1;

	if cmd == F_SETFD && arg == FD_CLOEXEC {
		0
	} else if cmd == F_GETFL {
		let obj = get_object(fd);
		obj.map_or_else(
			|e| -i32::from(e),
			|v| {
				block_on((*v).status_flags(), None)
					.map_or_else(|e| -i32::from(e), |status_flags| status_flags.bits())
			},
		)
	} else if cmd == F_SETFL {
		let obj = get_object(fd);
		obj.map_or_else(
			|e| -i32::from(e),
			|v| {
				block_on(
					(*v).set_status_flags(fd::StatusFlags::from_bits_retain(arg)),
					None,
				)
				.map_or_else(|e| -i32::from(e), |()| 0)
			},
		)
	} else {
		-crate::errno::EINVAL
	}
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub extern "C" fn sys_lseek(fd: FileDescriptor, offset: isize, whence: i32) -> isize {
	let whence = u8::try_from(whence).unwrap();
	let whence = SeekWhence::try_from(whence).unwrap();
	crate::fd::lseek(fd, offset, whence).unwrap_or_else(|e| isize::try_from(-i32::from(e)).unwrap())
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
	pub d_name: PhantomData<c_char>,
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_getdents64(
	fd: FileDescriptor,
	dirp: *mut Dirent64,
	count: usize,
) -> i64 {
	if dirp.is_null() || count == 0 {
		return (-crate::errno::EINVAL).into();
	}

	const ALIGN_DIRENT: usize = core::mem::align_of::<Dirent64>();
	let mut dirp: *mut Dirent64 = dirp;
	let mut offset: i64 = 0;
	let obj = get_object(fd);
	obj.map_or_else(
		|_| (-crate::errno::EINVAL).into(),
		|v| {
			block_on((*v).readdir(), None).map_or_else(
				|e| i64::from(-i32::from(e)),
				|v| {
					for i in v.iter() {
						let len = i.name.len();
						let aligned_len = ((core::mem::size_of::<Dirent64>() + len + 1)
							+ (ALIGN_DIRENT - 1)) & (!(ALIGN_DIRENT - 1));
						if offset as usize + aligned_len >= count {
							return (-crate::errno::EINVAL).into();
						}

						let dir = unsafe { &mut *dirp };

						dir.d_ino = 0;
						dir.d_type = 0;
						dir.d_reclen = aligned_len.try_into().unwrap();
						offset += i64::try_from(aligned_len).unwrap();
						dir.d_off = offset;

						// copy null-terminated filename
						let s = ptr::from_mut(&mut dir.d_name).cast::<u8>();
						unsafe {
							core::ptr::copy_nonoverlapping(i.name.as_ptr(), s, len);
							s.add(len).write_bytes(0, 1);
						}

						dirp = unsafe { dirp.byte_add(aligned_len) };
					}

					offset
				},
			)
		},
	)
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub extern "C" fn sys_dup(fd: i32) -> i32 {
	dup_object(fd).unwrap_or_else(|e| -i32::from(e))
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub extern "C" fn sys_dup2(fd1: i32, fd2: i32) -> i32 {
	dup_object2(fd1, fd2).unwrap_or_else(|e| -i32::from(e))
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub extern "C" fn sys_isatty(fd: i32) -> i32 {
	match isatty(fd) {
		Err(e) => -i32::from(e),
		Ok(v) => {
			if v {
				1
			} else {
				0
			}
		}
	}
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_poll(fds: *mut PollFd, nfds: usize, timeout: i32) -> i32 {
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
			if e == io::Error::ETIME {
				0
			} else {
				-i32::from(e)
			}
		},
		|v| v.try_into().unwrap(),
	)
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub extern "C" fn sys_eventfd(initval: u64, flags: i16) -> i32 {
	if let Some(flags) = EventFlags::from_bits(flags) {
		crate::fd::eventfd(initval, flags).unwrap_or_else(|e| -i32::from(e))
	} else {
		-crate::errno::EINVAL
	}
}

#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_image_start_addr() -> usize {
	crate::mm::kernel_start_address().as_usize()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[cfg(target_os = "none")]
	#[test_case]
	fn test_get_application_parameters() {
		crate::env::init();
		let (argc, argv, _envp) = get_application_parameters();
		assert_ne!(argc, 0);
		assert_ne!(argv, ptr::null());
	}
}
