use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::future::{self, Future};
use core::sync::atomic::{AtomicI32, Ordering};
use core::task::Poll::{Pending, Ready};
use core::time::Duration;

use ahash::RandomState;
use async_trait::async_trait;
use dyn_clone::DynClone;
use hashbrown::HashMap;
#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "newlib")))]
use smoltcp::wire::{IpEndpoint, IpListenEndpoint};

use crate::env;
use crate::executor::{block_on, poll_on};
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
static OBJECT_MAP: async_lock::RwLock<
	HashMap<FileDescriptor, Arc<dyn ObjectInterface>, RandomState>,
> = async_lock::RwLock::new(HashMap::<
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
	#[derive(Debug, Copy, Clone, Default)]
	pub struct PollEvent: i16 {
		const EMPTY = 0;
		const POLLIN = 0x1;
		const POLLPRI = 0x2;
		const POLLOUT = 0x4;
		const POLLERR = 0x8;
		const POLLHUP = 0x10;
		const POLLNVAL = 0x20;
		const POLLRDNORM = 0x040;
		const POLLRDBAND = 0x080;
		const POLLWRNORM = 0x0100;
		const POLLWRBAND = 0x0200;
		const POLLRDHUP = 0x2000;
	}
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct PollFd {
	/// file descriptor
	pub fd: i32,
	/// events to look for
	pub events: PollEvent,
	/// events returned
	pub revents: PollEvent,
}

bitflags! {
	#[derive(Debug, Copy, Clone)]
	pub struct AccessPermission: u32 {
		const S_IFMT = 0o170000;
		const S_IFSOCK = 0o140000;
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

#[async_trait]
pub(crate) trait ObjectInterface: Sync + Send + core::fmt::Debug + DynClone {
	/// check if an IO event is possible
	async fn poll(&self, _event: PollEvent) -> Result<PollEvent, IoError> {
		Ok(PollEvent::EMPTY)
	}

	/// `async_read` attempts to read `len` bytes from the object references
	/// by the descriptor
	async fn async_read(&self, _buf: &mut [u8]) -> Result<usize, IoError> {
		Err(IoError::ENOSYS)
	}

	/// `async_write` attempts to write `len` bytes to the object references
	/// by the descriptor
	async fn async_write(&self, _buf: &[u8]) -> Result<usize, IoError> {
		Err(IoError::ENOSYS)
	}

	/// `is_nonblocking` returns `true`, if `read`, `write`, `recv` and send operations
	/// don't block.
	fn is_nonblocking(&self) -> bool {
		false
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
	fn readdir(&self) -> Result<Vec<DirectoryEntry>, IoError> {
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
		block_on(
			async {
				if OBJECT_MAP.write().await.try_insert(fd, file).is_err() {
					Err(IoError::EINVAL)
				} else {
					Ok(fd as FileDescriptor)
				}
			},
			None,
		)
	} else {
		Err(IoError::EINVAL)
	}
}

pub(crate) fn close(fd: FileDescriptor) {
	let _ = remove_object(fd).map(|v| v.close());
}

pub(crate) fn read(fd: FileDescriptor, buf: &mut [u8]) -> Result<usize, IoError> {
	let obj = get_object(fd)?;

	if buf.is_empty() {
		return Ok(0);
	}

	if obj.is_nonblocking() {
		poll_on(obj.async_read(buf), Some(Duration::ZERO.into())).map_err(|x| {
			if x == IoError::ETIME {
				IoError::EAGAIN
			} else {
				x
			}
		})
	} else {
		match poll_on(obj.async_read(buf), Some(Duration::from_secs(2).into())) {
			Err(IoError::ETIME) => block_on(obj.async_read(buf), None),
			Err(x) => Err(x),
			Ok(x) => Ok(x),
		}
	}
}

pub(crate) fn write(fd: FileDescriptor, buf: &[u8]) -> Result<usize, IoError> {
	let obj = get_object(fd)?;

	if buf.is_empty() {
		return Ok(0);
	}

	if obj.is_nonblocking() {
		poll_on(obj.async_write(buf), Some(Duration::ZERO.into())).map_err(|x| {
			if x == IoError::ETIME {
				IoError::EAGAIN
			} else {
				x
			}
		})
	} else {
		match poll_on(obj.async_write(buf), Some(Duration::from_secs(2).into())) {
			Err(IoError::ETIME) => block_on(obj.async_write(buf), None),
			Err(x) => Err(x),
			Ok(x) => Ok(x),
		}
	}
}

async fn poll_fds(fds: &mut [PollFd]) -> Result<(), IoError> {
	future::poll_fn(|cx| {
		let mut ready: bool = false;

		for i in &mut *fds {
			let fd = i.fd;
			let mut pinned_obj = core::pin::pin!(async_get_object(fd));
			if let Ready(Ok(obj)) = pinned_obj.as_mut().poll(cx) {
				let mut pinned = core::pin::pin!(obj.poll(i.events));
				if let Ready(Ok(e)) = pinned.as_mut().poll(cx) {
					ready = true;
					i.revents = e;
				}
			}
		}

		if ready {
			Ready(())
		} else {
			Pending
		}
	})
	.await;

	Ok(())
}

pub(crate) fn poll(fds: &mut [PollFd], timeout: i32) -> Result<(), IoError> {
	if timeout >= 0 {
		// for larger timeouts, we block on the async function
		if timeout >= 5000 {
			block_on(
				poll_fds(fds),
				Some(Duration::from_millis(timeout.try_into().unwrap())),
			)
		} else {
			poll_on(
				poll_fds(fds),
				Some(Duration::from_millis(timeout.try_into().unwrap())),
			)
		}
	} else {
		block_on(poll_fds(fds), None)
	}
}

#[inline]
async fn async_get_object(fd: FileDescriptor) -> Result<Arc<dyn ObjectInterface>, IoError> {
	Ok((*(OBJECT_MAP.read().await.get(&fd).ok_or(IoError::EINVAL)?)).clone())
}

pub(crate) fn get_object(fd: FileDescriptor) -> Result<Arc<dyn ObjectInterface>, IoError> {
	block_on(async_get_object(fd), None)
}

#[inline]
async fn async_insert_object(
	fd: FileDescriptor,
	obj: Arc<dyn ObjectInterface>,
) -> Result<(), IoError> {
	let _ = OBJECT_MAP.write().await.insert(fd, obj);
	Ok(())
}

pub(crate) fn insert_object(
	fd: FileDescriptor,
	obj: Arc<dyn ObjectInterface>,
) -> Result<(), IoError> {
	block_on(async_insert_object(fd, obj), None)
}

// The dup system call allocates a new file descriptor that refers
// to the same open file description as the descriptor oldfd. The new
// file descriptor number is guaranteed to be the lowest-numbered
// file descriptor that was unused in the calling process.
pub(crate) fn dup_object(fd: FileDescriptor) -> Result<FileDescriptor, IoError> {
	block_on(
		async {
			let mut guard = OBJECT_MAP.write().await;
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
		},
		None,
	)
}

pub(crate) fn remove_object(fd: FileDescriptor) -> Result<Arc<dyn ObjectInterface>, IoError> {
	block_on(
		async {
			if fd <= 2 {
				Err(IoError::EINVAL)
			} else {
				let obj = OBJECT_MAP
					.write()
					.await
					.remove(&fd)
					.ok_or(IoError::EINVAL)?;
				Ok(obj)
			}
		},
		None,
	)
}

pub(crate) fn init() -> Result<(), IoError> {
	block_on(
		async {
			let mut guard = OBJECT_MAP.write().await;
			if env::is_uhyve() {
				guard
					.try_insert(STDIN_FILENO, Arc::new(UhyveStdin::new()))
					.map_err(|_| IoError::EIO)?;
				guard
					.try_insert(STDOUT_FILENO, Arc::new(UhyveStdout::new()))
					.map_err(|_| IoError::EIO)?;
				guard
					.try_insert(STDERR_FILENO, Arc::new(UhyveStderr::new()))
					.map_err(|_| IoError::EIO)?;
			} else {
				guard
					.try_insert(STDIN_FILENO, Arc::new(GenericStdin::new()))
					.map_err(|_| IoError::EIO)?;
				guard
					.try_insert(STDOUT_FILENO, Arc::new(GenericStdout::new()))
					.map_err(|_| IoError::EIO)?;
				guard
					.try_insert(STDERR_FILENO, Arc::new(GenericStderr::new()))
					.map_err(|_| IoError::EIO)?;
			}

			Ok(())
		},
		None,
	)
}
