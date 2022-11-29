use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use core::ffi::CStr;
use core::sync::atomic::{AtomicI32, Ordering};

use hermit_sync::InterruptTicketMutex;

use crate::arch::mm::VirtAddr;
use crate::env;
use crate::errno::*;
use crate::fd::file::{GenericFile, UhyveFile};
use crate::fd::interfaces::{uhyve_send, SysOpen, SyscallInterface, UHYVE_PORT_OPEN};
use crate::fd::stdio::*;
use crate::syscalls::fs::{self, FilePerms};

mod file;
pub(crate) mod interfaces;
mod stdio;

pub const STDIN_FILENO: FileDescriptor = 0;
pub const STDOUT_FILENO: FileDescriptor = 1;
pub const STDERR_FILENO: FileDescriptor = 2;

pub(crate) type FileDescriptor = i32;

pub(crate) static mut SYS: &'static dyn SyscallInterface = &self::interfaces::Generic;
static OBJECT_MAP: InterruptTicketMutex<BTreeMap<FileDescriptor, Arc<dyn ObjectInterface>>> =
	InterruptTicketMutex::new(BTreeMap::new());
static FD_COUNTER: AtomicI32 = AtomicI32::new(3);

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

pub trait ObjectInterface: Sync + Send + core::fmt::Debug {
	/// `read` attempts to read `len` bytes from the object references
	/// by the descriptor
	fn read(&self, _buf: *mut u8, _len: usize) -> isize {
		(-ENOSYS).try_into().unwrap()
	}

	/// `write` attempts to write `len` bytes to the object references
	/// by the descriptor
	fn write(&self, _buf: *const u8, _len: usize) -> isize {
		(-ENOSYS).try_into().unwrap()
	}

	/// close a file descriptor
	fn close(&self) -> i32 {
		-ENOSYS
	}

	/// `lseek` function repositions the offset of the file descriptor fildes
	fn lseek(&self, _offset: isize, _whence: i32) -> isize {
		(-ENOSYS).try_into().unwrap()
	}
}

pub(crate) fn open(name: *const u8, flags: i32, mode: i32) -> Result<FileDescriptor, i32> {
	if env::is_uhyve() {
		let mut sysopen = SysOpen::new(VirtAddr(name as u64), flags, mode);
		uhyve_send(UHYVE_PORT_OPEN, &mut sysopen);

		if sysopen.ret > 0 {
			let fd = FD_COUNTER.fetch_add(1, Ordering::SeqCst);
			let file = UhyveFile::new(sysopen.ret);

			if OBJECT_MAP.lock().try_insert(fd, Arc::new(file)).is_err() {
				Err(-EINVAL)
			} else {
				Ok(fd as FileDescriptor)
			}
		} else {
			Err(sysopen.ret)
		}
	} else {
		#[cfg(target_arch = "x86_64")]
		{
			// mode is 0x777 (0b0111_0111_0111), when flags | O_CREAT, else 0
			// flags is bitmask of O_DEC_* defined above.
			// (taken from rust stdlib/sys hermit target )

			let name = unsafe { CStr::from_ptr(name as _) }.to_str().unwrap();
			debug!("Open {}, {}, {}", name, flags, mode);

			let mut fs = fs::FILESYSTEM.lock();
			if let Ok(filesystem_fd) = fs.open(name, open_flags_to_perm(flags, mode as u32)) {
				let fd = FD_COUNTER.fetch_add(1, Ordering::SeqCst);
				let file = GenericFile::new(filesystem_fd);
				if OBJECT_MAP.lock().try_insert(fd, Arc::new(file)).is_err() {
					Err(-EINVAL)
				} else {
					Ok(fd as FileDescriptor)
				}
			} else {
				Err(-EINVAL)
			}
		}
		#[cfg(not(target_arch = "x86_64"))]
		{
			Err(-ENOSYS)
		}
	}
}

pub(crate) fn get_object(fd: FileDescriptor) -> Result<Arc<dyn ObjectInterface>, i32> {
	Ok((*(OBJECT_MAP.lock().get(&fd).ok_or(-EINVAL)?)).clone())
}

pub(crate) fn remove_object(fd: FileDescriptor) {
	if fd > 2 {
		if OBJECT_MAP.lock().remove(&fd).is_none() {
			debug!("Unable to remove object {}", fd);
		}
	}
}

pub(crate) fn init() {
	unsafe {
		// We know that HermitCore has successfully initialized a network interface.
		// Now check if we can load a more specific SyscallInterface to make use of networking.
		if env::is_uhyve() {
			SYS = &interfaces::Uhyve;
		}

		// Perform interface-specific initialization steps.
		SYS.init();
	}

	let mut guard = OBJECT_MAP.lock();
	if env::is_uhyve() {
		guard
			.try_insert(STDIN_FILENO, Arc::new(UhyveStdin::new()))
			.unwrap();
		guard
			.try_insert(STDOUT_FILENO, Arc::new(UhyveStdout::new()))
			.unwrap();
		guard
			.try_insert(STDERR_FILENO, Arc::new(UhyveStderr::new()))
			.unwrap();
	} else {
		guard
			.try_insert(STDIN_FILENO, Arc::new(GenericStdin::new()))
			.unwrap();
		guard
			.try_insert(STDOUT_FILENO, Arc::new(GenericStdout::new()))
			.unwrap();
		guard
			.try_insert(STDERR_FILENO, Arc::new(GenericStderr::new()))
			.unwrap();
	}
}
