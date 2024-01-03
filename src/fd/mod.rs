use alloc::sync::Arc;
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, Ordering};

use ahash::RandomState;
use dyn_clone::DynClone;
use hashbrown::HashMap;

use crate::env;
use crate::errno::*;
use crate::fd::stdio::*;
use crate::fs::{self, FileAttr, SeekWhence};
#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
use crate::syscalls::net::*;

#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
pub(crate) mod socket;
mod stdio;

const STDIN_FILENO: FileDescriptor = 0;
const STDOUT_FILENO: FileDescriptor = 1;
const STDERR_FILENO: FileDescriptor = 2;

// TODO: Integrate with src/errno.rs ?
#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, PartialEq, FromPrimitive, ToPrimitive)]
pub(crate) enum IoError {
	ENOENT = crate::errno::ENOENT as isize,
	ENOSYS = crate::errno::ENOSYS as isize,
	EIO = crate::errno::EIO as isize,
	EBADF = crate::errno::EBADF as isize,
	EISDIR = crate::errno::EISDIR as isize,
	EINVAL = crate::errno::EINVAL as isize,
	ETIME = crate::errno::ETIME as isize,
	EAGAIN = crate::errno::EAGAIN as isize,
	EFAULT = crate::errno::EFAULT as isize,
	ENOBUFS = crate::errno::ENOBUFS as isize,
	ENOTCONN = crate::errno::ENOTCONN as isize,
	ENOTDIR = crate::errno::ENOTDIR as isize,
	EMFILE = crate::errno::EMFILE as isize,
	EEXIST = crate::errno::EEXIST as isize,
}

pub(crate) type FileDescriptor = i32;

/// Mapping between file descriptor and the referenced object
static OBJECT_MAP: pflock::PFLock<HashMap<FileDescriptor, Arc<dyn ObjectInterface>, RandomState>> =
	pflock::PFLock::new(HashMap::<
		FileDescriptor,
		Arc<dyn ObjectInterface>,
		RandomState,
	>::with_hasher(RandomState::with_seeds(0, 0, 0, 0)));
/// Atomic counter to determine the next unused file descriptor
static FD_COUNTER: AtomicI32 = AtomicI32::new(3);

bitflags! {
	/// Options for opening files
	#[derive(Debug, Copy, Clone, Default)]
	pub(crate) struct OpenOption: i32 {
		const O_RDONLY = 0o0000;
		const O_WRONLY = 0o0001;
		const O_RDWR = 0o0002;
		const O_CREAT = 0o0100;
		const O_EXCL = 0o0200;
		const O_TRUNC = 0o1000;
		const O_APPEND = 0o2000;
		const O_DIRECT = 0o40000;
	}
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct Dirent {
	pub d_ino: u64,
	pub d_off: u64,
	pub d_namelen: u32,
	pub d_type: u32,
	pub d_name: [u8; 0],
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub enum DirectoryEntry {
	Invalid(i32),
	Valid(*const Dirent),
}

pub(crate) trait ObjectInterface: Sync + Send + core::fmt::Debug + DynClone {
	/// `read` attempts to read `len` bytes from the object references
	/// by the descriptor
	fn read(&self, _buf: &mut [u8]) -> Result<isize, IoError> {
		Err(IoError::ENOSYS)
	}

	/// `write` attempts to write `len` bytes to the object references
	/// by the descriptor
	fn write(&self, _buf: &[u8]) -> Result<isize, IoError> {
		Err(IoError::ENOSYS)
	}

	/// `lseek` function repositions the offset of the file descriptor fildes
	fn lseek(&self, _offset: isize, _whence: SeekWhence) -> Result<isize, IoError> {
		Err(IoError::EINVAL)
	}

	/// `fstat`
	fn fstat(&self, _stat: &mut FileAttr) -> Result<(), IoError> {
		Err(IoError::EINVAL)
	}

	/// `unlink` removes file entry
	fn unlink(&self, _path: &str) -> Result<(), IoError> {
		Err(IoError::EINVAL)
	}

	/// `rmdir` removes directory entry
	fn rmdir(&self, _path: &str) -> Result<(), IoError> {
		Err(IoError::EINVAL)
	}

	/// 'readdir' returns a pointer to a dirent structure
	/// representing the next directory entry in the directory stream
	/// pointed to by the file descriptor
	fn readdir(&self) -> DirectoryEntry {
		DirectoryEntry::Invalid(-ENOSYS)
	}

	/// `mkdir` creates a directory entry
	fn mkdir(&self, _path: &str, _mode: u32) -> Result<(), IoError> {
		Err(IoError::EINVAL)
	}

	/// `accept` a connection on a socket
	#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
	fn accept(&self, _addr: *mut sockaddr, _addrlen: *mut socklen_t) -> Result<i32, IoError> {
		Err(IoError::EINVAL)
	}

	/// initiate a connection on a socket
	#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
	fn connect(&self, _name: *const sockaddr, _namelen: socklen_t) -> Result<i32, IoError> {
		Err(IoError::EINVAL)
	}

	/// `bind` a name to a socket
	#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
	fn bind(&self, _name: *const sockaddr, _namelen: socklen_t) -> Result<(), IoError> {
		Err(IoError::EINVAL)
	}

	/// `listen` for connections on a socket
	#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
	fn listen(&self, _backlog: i32) -> Result<(), IoError> {
		Err(IoError::EINVAL)
	}

	/// `setsockopt` sets options on sockets
	#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
	fn setsockopt(
		&self,
		_level: i32,
		_optname: i32,
		_optval: *const c_void,
		_optlen: socklen_t,
	) -> Result<(), IoError> {
		Err(IoError::EINVAL)
	}

	/// `getsockopt` gets options on sockets
	#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
	fn getsockopt(
		&self,
		_level: i32,
		_option_name: i32,
		_optval: *mut c_void,
		_optlen: *mut socklen_t,
	) -> Result<(), IoError> {
		Err(IoError::EINVAL)
	}

	/// `getsockname` gets socket name
	#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
	fn getsockname(&self, _name: *mut sockaddr, _namelen: *mut socklen_t) -> Result<(), IoError> {
		Err(IoError::EINVAL)
	}

	/// `getpeername` get address of connected peer
	#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
	fn getpeername(&self, _name: *mut sockaddr, _namelen: *mut socklen_t) -> Result<(), IoError> {
		Err(IoError::EINVAL)
	}

	/// receive a message from a socket
	///
	/// If `address` is not a null pointer, the source address of the message is filled in.  The
	/// `address_len` argument is a value-result argument, initialized to the size
	/// of the buffer associated with address, and modified on return to
	/// indicate the actual size of the address stored there.
	#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
	fn recvfrom(
		&self,
		_buffer: &mut [u8],
		_address: *mut sockaddr,
		_address_len: *mut socklen_t,
	) -> Result<isize, IoError> {
		Err(IoError::ENOSYS)
	}

	/// send a message from a socket
	///
	/// The sendto() function shall send a message.
	/// If the socket is a connectionless-mode socket, the message shall
	/// If a peer address has been prespecified, either the message shall
	/// be sent to the address specified by dest_addr (overriding the pre-specified peer
	/// address).
	#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
	fn sendto(
		&self,
		_buffer: &[u8],
		_addr: *const sockaddr,
		_addr_len: socklen_t,
	) -> Result<isize, IoError> {
		Err(IoError::ENOSYS)
	}

	/// shut down part of a full-duplex connection
	#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
	fn shutdown(&self, _how: i32) -> Result<(), IoError> {
		Err(IoError::ENOSYS)
	}

	/// The `ioctl` function manipulates the underlying device parameters of special
	/// files.
	fn ioctl(&self, _cmd: i32, _argp: *mut c_void) -> Result<(), IoError> {
		Err(IoError::ENOSYS)
	}

	// close a file descriptor
	fn close(&self) {
		trace!("close file descriptor");
	}
}

pub(crate) fn open(name: &str, flags: i32, mode: i32) -> Result<FileDescriptor, IoError> {
	// mode is 0x777 (0b0111_0111_0111), when flags | O_CREAT, else 0
	// flags is bitmask of O_DEC_* defined above.
	// (taken from rust stdlib/sys hermit target )

	debug!("Open {}, {}, {}", name, flags, mode);

	let fs = fs::FILESYSTEM.get().unwrap();
	if let Ok(file) = fs.open(
		name,
		OpenOption::from_bits(flags).expect("Invalid open flags"),
	) {
		let fd = FD_COUNTER.fetch_add(1, Ordering::SeqCst);
		if OBJECT_MAP.write().try_insert(fd, file).is_err() {
			Err(IoError::EINVAL)
		} else {
			Ok(fd as FileDescriptor)
		}
	} else {
		Err(IoError::EINVAL)
	}
}

#[allow(unused_variables)]
pub(crate) fn opendir(name: &str) -> Result<FileDescriptor, IoError> {
	debug!("Open directory {}", name);

	let fs = fs::FILESYSTEM.get().unwrap();
	if let Ok(obj) = fs.opendir(name) {
		let fd = FD_COUNTER.fetch_add(1, Ordering::SeqCst);
		// Would a GenericDir make sense?
		if OBJECT_MAP.write().try_insert(fd, obj).is_err() {
			Err(IoError::EINVAL)
		} else {
			Ok(fd as FileDescriptor)
		}
	} else {
		Err(IoError::EINVAL)
	}
}

pub(crate) fn get_object(fd: FileDescriptor) -> Result<Arc<dyn ObjectInterface>, IoError> {
	Ok((*(OBJECT_MAP.read().get(&fd).ok_or(IoError::EINVAL)?)).clone())
}

#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
pub(crate) fn insert_object(
	fd: FileDescriptor,
	obj: Arc<dyn ObjectInterface>,
) -> Option<Arc<dyn ObjectInterface>> {
	OBJECT_MAP.write().insert(fd, obj)
}

// The dup system call allocates a new file descriptor that refers
// to the same open file description as the descriptor oldfd. The new
// file descriptor number is guaranteed to be the lowest-numbered
// file descriptor that was unused in the calling process.
pub(crate) fn dup_object(fd: FileDescriptor) -> Result<FileDescriptor, IoError> {
	let mut guard = OBJECT_MAP.write();
	let obj = (*(guard.get(&fd).ok_or(IoError::EINVAL)?)).clone();

	let new_fd = || -> i32 {
		for i in 3..FD_COUNTER.load(Ordering::SeqCst) {
			if !guard.contains_key(&i) {
				return i;
			}
		}
		FD_COUNTER.fetch_add(1, Ordering::SeqCst)
	};

	let fd = new_fd();
	if guard.try_insert(fd, obj).is_err() {
		Err(IoError::EMFILE)
	} else {
		Ok(fd as FileDescriptor)
	}
}

pub(crate) fn remove_object(fd: FileDescriptor) -> Result<Arc<dyn ObjectInterface>, IoError> {
	if fd <= 2 {
		Err(IoError::EINVAL)
	} else {
		let obj = OBJECT_MAP.write().remove(&fd).ok_or(IoError::EINVAL)?;
		Ok(obj)
	}
}

pub(crate) fn init() {
	let mut guard = OBJECT_MAP.write();
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
