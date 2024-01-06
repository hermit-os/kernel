use alloc::sync::Arc;
use core::sync::atomic::{AtomicI32, Ordering};

use ahash::RandomState;
use dyn_clone::DynClone;
use hashbrown::HashMap;
#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
use smoltcp::wire::{IpEndpoint, IpListenEndpoint};

use crate::env;
use crate::fd::stdio::*;
use crate::fs::{self, DirectoryEntry, FileAttr, SeekWhence};

#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
pub(crate) mod socket;
mod stdio;

const STDIN_FILENO: FileDescriptor = 0;
const STDOUT_FILENO: FileDescriptor = 1;
const STDERR_FILENO: FileDescriptor = 2;

// TODO: Integrate with src/errno.rs ?
#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, PartialEq, FromPrimitive, ToPrimitive)]
pub enum IoError {
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
	EADDRINUSE = crate::errno::EADDRINUSE as isize,
}

#[allow(dead_code)]
#[derive(Debug, PartialEq)]
pub(crate) enum SocketOption {
	TcpNoDelay,
}

#[allow(dead_code)]
#[derive(Debug, PartialEq)]
pub(crate) enum IoCtl {
	NonBlocking,
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
pub(crate) static FD_COUNTER: AtomicI32 = AtomicI32::new(3);

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

bitflags! {
	#[derive(Debug, Copy, Clone)]
	pub struct AccessPermission: u32 {
		const S_IFMT = 0o170000;
		const S_IFSOCK = 0140000;
		const S_IFLNK = 0o120000;
		const S_IFREG = 0o100000;
		const S_IFBLK = 0o060000;
		const S_IFDIR = 0o040000;
		const S_IFCHR = 0o020000;
		const S_IFIFO = 0o010000;
		const S_IRUSR = 0o400;
		const S_IWUSR = 0o200;
		const S_IXUSR = 0o100;
		const S_IRWXU = 0o700;
		const S_IRGRP = 0o040;
		const S_IWGRP = 0o020;
		const S_IXGRP = 0o010;
		const S_IRWXG = 0o070;
		const S_IROTH = 0o004;
		const S_IWOTH = 0o002;
		const S_IXOTH = 0o001;
		const S_IRWXO = 0o007;
	}
}

impl Default for AccessPermission {
	fn default() -> Self {
		AccessPermission::from_bits(0o666).unwrap()
	}
}

pub(crate) trait ObjectInterface: Sync + Send + core::fmt::Debug + DynClone {
	/// `read` attempts to read `len` bytes from the object references
	/// by the descriptor
	fn read(&self, _buf: &mut [u8]) -> Result<usize, IoError> {
		Err(IoError::ENOSYS)
	}

	/// `write` attempts to write `len` bytes to the object references
	/// by the descriptor
	fn write(&self, _buf: &[u8]) -> Result<usize, IoError> {
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
	fn readdir(&self) -> Result<Option<DirectoryEntry>, IoError> {
		Err(IoError::EINVAL)
	}

	/// `mkdir` creates a directory entry
	fn mkdir(&self, _path: &str, _mode: u32) -> Result<(), IoError> {
		Err(IoError::EINVAL)
	}

	/// `accept` a connection on a socket
	#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
	fn accept(&self) -> Result<IpEndpoint, IoError> {
		Err(IoError::EINVAL)
	}

	/// initiate a connection on a socket
	#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
	fn connect(&self, _endpoint: IpEndpoint) -> Result<(), IoError> {
		Err(IoError::EINVAL)
	}

	/// `bind` a name to a socket
	#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
	fn bind(&self, _name: IpListenEndpoint) -> Result<(), IoError> {
		Err(IoError::EINVAL)
	}

	/// `listen` for connections on a socket
	#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
	fn listen(&self, _backlog: i32) -> Result<(), IoError> {
		Err(IoError::EINVAL)
	}

	/// `setsockopt` sets options on sockets
	#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
	fn setsockopt(&self, _opt: SocketOption, _optval: bool) -> Result<(), IoError> {
		Err(IoError::EINVAL)
	}

	/// `getsockopt` gets options on sockets
	#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
	fn getsockopt(&self, _opt: SocketOption) -> Result<bool, IoError> {
		Err(IoError::EINVAL)
	}

	/// `getsockname` gets socket name
	#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
	fn getsockname(&self) -> Option<IpEndpoint> {
		None
	}

	/// `getpeername` get address of connected peer
	#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
	fn getpeername(&self) -> Option<IpEndpoint> {
		None
	}

	/// receive a message from a socket
	#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
	fn recvfrom(&self, _buffer: &mut [u8]) -> Result<(usize, IpEndpoint), IoError> {
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
	fn sendto(&self, _buffer: &[u8], _endpoint: IpEndpoint) -> Result<usize, IoError> {
		Err(IoError::ENOSYS)
	}

	/// shut down part of a full-duplex connection
	#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
	fn shutdown(&self, _how: i32) -> Result<(), IoError> {
		Err(IoError::ENOSYS)
	}

	/// The `ioctl` function manipulates the underlying device parameters of special
	/// files.
	fn ioctl(&self, _cmd: IoCtl, _value: bool) -> Result<(), IoError> {
		Err(IoError::ENOSYS)
	}

	// close a file descriptor
	fn close(&self) {}
}

pub(crate) fn open(
	name: &str,
	flags: OpenOption,
	mode: AccessPermission,
) -> Result<FileDescriptor, IoError> {
	// mode is 0x777 (0b0111_0111_0111), when flags | O_CREAT, else 0
	// flags is bitmask of O_DEC_* defined above.
	// (taken from rust stdlib/sys hermit target )

	debug!("Open {}, {:?}, {:?}", name, flags, mode);

	let fs = fs::FILESYSTEM.get().unwrap();
	if let Ok(file) = fs.open(name, flags, mode) {
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

pub(crate) fn close(fd: FileDescriptor) {
	let _ = remove_object(fd).map(|v| v.close());
}

pub(crate) fn read(fd: FileDescriptor, buf: &mut [u8]) -> Result<usize, IoError> {
	get_object(fd)?.read(buf)
}

pub(crate) fn write(fd: FileDescriptor, buf: &[u8]) -> Result<usize, IoError> {
	get_object(fd)?.write(buf)
}

pub(crate) fn get_object(fd: FileDescriptor) -> Result<Arc<dyn ObjectInterface>, IoError> {
	Ok((*(OBJECT_MAP.read().get(&fd).ok_or(IoError::EINVAL)?)).clone())
}

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
