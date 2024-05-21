use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::future::{self, Future};
use core::task::Poll::{Pending, Ready};
use core::time::Duration;

use async_trait::async_trait;
use dyn_clone::DynClone;
#[cfg(any(feature = "tcp", feature = "udp"))]
use smoltcp::wire::{IpEndpoint, IpListenEndpoint};

use crate::arch::kernel::core_local::core_scheduler;
use crate::executor::{block_on, poll_on};
use crate::fs::{DirectoryEntry, FileAttr, SeekWhence};

mod eventfd;
#[cfg(any(feature = "tcp", feature = "udp"))]
pub(crate) mod socket;
pub(crate) mod stdio;

pub(crate) const STDIN_FILENO: FileDescriptor = 0;
pub(crate) const STDOUT_FILENO: FileDescriptor = 1;
pub(crate) const STDERR_FILENO: FileDescriptor = 2;

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
	EOVERFLOW = crate::errno::EOVERFLOW as isize,
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

bitflags! {
	/// Options for opening files
	#[derive(Debug, Copy, Clone, Default)]
	pub struct OpenOption: i32 {
		const O_RDONLY = 0o0000;
		const O_WRONLY = 0o0001;
		const O_RDWR = 0o0002;
		const O_CREAT = 0o0100;
		const O_EXCL = 0o0200;
		const O_TRUNC = 0o1000;
		const O_APPEND = 0o2000;
		const O_DIRECT = 0o40000;
		const O_DIRECTORY = 0o200000;
	}
}

bitflags! {
	#[derive(Debug, Copy, Clone, Default)]
	pub struct PollEvent: i16 {
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
	#[derive(Debug, Default, Copy, Clone)]
	pub struct EventFlags: i16 {
		const EFD_SEMAPHORE = 0o1;
		const EFD_NONBLOCK = 0o4000;
		const EFD_CLOEXEC = 0o40000;
	}
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
		// Allow bits unknown to us to be set externally. See bitflags documentation for further explanation.
		const _ = !0;
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
		Ok(PollEvent::empty())
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
	#[allow(dead_code)]
	fn unlink(&self, _path: &str) -> Result<(), IoError> {
		Err(IoError::EINVAL)
	}

	/// `rmdir` removes directory entry
	#[allow(dead_code)]
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
	#[allow(dead_code)]
	fn mkdir(&self, _path: &str, _mode: u32) -> Result<(), IoError> {
		Err(IoError::EINVAL)
	}

	/// `accept` a connection on a socket
	#[cfg(any(feature = "tcp", feature = "udp"))]
	fn accept(&self) -> Result<IpEndpoint, IoError> {
		Err(IoError::EINVAL)
	}

	/// initiate a connection on a socket
	#[cfg(any(feature = "tcp", feature = "udp"))]
	fn connect(&self, _endpoint: IpEndpoint) -> Result<(), IoError> {
		Err(IoError::EINVAL)
	}

	/// `bind` a name to a socket
	#[cfg(any(feature = "tcp", feature = "udp"))]
	fn bind(&self, _name: IpListenEndpoint) -> Result<(), IoError> {
		Err(IoError::EINVAL)
	}

	/// `listen` for connections on a socket
	#[cfg(any(feature = "tcp", feature = "udp"))]
	fn listen(&self, _backlog: i32) -> Result<(), IoError> {
		Err(IoError::EINVAL)
	}

	/// `setsockopt` sets options on sockets
	#[cfg(any(feature = "tcp", feature = "udp"))]
	fn setsockopt(&self, _opt: SocketOption, _optval: bool) -> Result<(), IoError> {
		Err(IoError::EINVAL)
	}

	/// `getsockopt` gets options on sockets
	#[cfg(any(feature = "tcp", feature = "udp"))]
	fn getsockopt(&self, _opt: SocketOption) -> Result<bool, IoError> {
		Err(IoError::EINVAL)
	}

	/// `getsockname` gets socket name
	#[cfg(any(feature = "tcp", feature = "udp"))]
	fn getsockname(&self) -> Option<IpEndpoint> {
		None
	}

	/// `getpeername` get address of connected peer
	#[cfg(any(feature = "tcp", feature = "udp"))]
	#[allow(dead_code)]
	fn getpeername(&self) -> Option<IpEndpoint> {
		None
	}

	/// receive a message from a socket
	#[cfg(any(feature = "tcp", feature = "udp"))]
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
	#[cfg(any(feature = "tcp", feature = "udp"))]
	fn sendto(&self, _buffer: &[u8], _endpoint: IpEndpoint) -> Result<usize, IoError> {
		Err(IoError::ENOSYS)
	}

	/// shut down part of a full-duplex connection
	#[cfg(any(feature = "tcp", feature = "udp"))]
	fn shutdown(&self, _how: i32) -> Result<(), IoError> {
		Err(IoError::ENOSYS)
	}

	/// The `ioctl` function manipulates the underlying device parameters of special
	/// files.
	fn ioctl(&self, _cmd: IoCtl, _value: bool) -> Result<(), IoError> {
		Err(IoError::ENOSYS)
	}
}

pub(crate) fn read(fd: FileDescriptor, buf: &mut [u8]) -> Result<usize, IoError> {
	let obj = get_object(fd)?;

	if buf.is_empty() {
		return Ok(0);
	}

	if obj.is_nonblocking() {
		poll_on(obj.async_read(buf), Some(Duration::ZERO)).map_err(|x| {
			if x == IoError::ETIME {
				IoError::EAGAIN
			} else {
				x
			}
		})
	} else {
		match poll_on(obj.async_read(buf), Some(Duration::from_secs(2))) {
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
		poll_on(obj.async_write(buf), Some(Duration::ZERO)).map_err(|x| {
			if x == IoError::ETIME {
				IoError::EAGAIN
			} else {
				x
			}
		})
	} else {
		match poll_on(obj.async_write(buf), Some(Duration::from_secs(2))) {
			Err(IoError::ETIME) => block_on(obj.async_write(buf), None),
			Err(x) => Err(x),
			Ok(x) => Ok(x),
		}
	}
}

async fn poll_fds(fds: &mut [PollFd]) -> Result<u64, IoError> {
	future::poll_fn(|cx| {
		let mut counter: u64 = 0;

		for i in &mut *fds {
			let fd = i.fd;
			i.revents = PollEvent::empty();
			let mut pinned_obj = core::pin::pin!(core_scheduler().get_object(fd));
			if let Ready(Ok(obj)) = pinned_obj.as_mut().poll(cx) {
				let mut pinned = core::pin::pin!(obj.poll(i.events));
				if let Ready(Ok(e)) = pinned.as_mut().poll(cx) {
					if !e.is_empty() {
						counter += 1;
						i.revents = e;
					}
				}
			}
		}

		if counter > 0 {
			Ready(Ok(counter))
		} else {
			Pending
		}
	})
	.await
}

/// The unix-like `poll` waits for one of a set of file descriptors
/// to become ready to perform I/O. The set of file descriptors to be
/// monitored is specified in the `fds` argument, which is an array
/// of structs of `PollFd`.
pub fn poll(fds: &mut [PollFd], timeout: Option<Duration>) -> Result<u64, IoError> {
	let result = block_on(poll_fds(fds), timeout);
	if let Err(ref e) = result {
		if timeout.is_some() {
			// A return value of zero indicates that the system call timed out
			if *e == IoError::EAGAIN {
				return Ok(0);
			}
		}
	}

	result
}

/// `eventfd` creates an linux-like "eventfd object" that can be used
/// as an event wait/notify mechanism by user-space applications, and by
/// the kernel to notify user-space applications of events. The
/// object contains an unsigned 64-bit integer counter
/// that is maintained by the kernel. This counter is initialized
/// with the value specified in the argument `initval`.
///
/// As its return value, `eventfd` returns a new file descriptor that
/// can be used to refer to the eventfd object.
///
/// The following values may be bitwise set in flags to change the
/// behavior of `eventfd`:
///
/// `EFD_NONBLOCK`: Set the file descriptor in non-blocking mode
/// `EFD_SEMAPHORE`: Provide semaphore-like semantics for reads
/// from the new file descriptor.
pub fn eventfd(initval: u64, flags: EventFlags) -> Result<FileDescriptor, IoError> {
	let obj = self::eventfd::EventFd::new(initval, flags);

	let fd = block_on(core_scheduler().insert_object(Arc::new(obj)), None)?;

	Ok(fd)
}

pub(crate) fn get_object(fd: FileDescriptor) -> Result<Arc<dyn ObjectInterface>, IoError> {
	block_on(core_scheduler().get_object(fd), None)
}

pub(crate) fn insert_object(obj: Arc<dyn ObjectInterface>) -> Result<FileDescriptor, IoError> {
	block_on(core_scheduler().insert_object(obj), None)
}

#[allow(dead_code)]
pub(crate) fn replace_object(
	fd: FileDescriptor,
	obj: Arc<dyn ObjectInterface>,
) -> Result<(), IoError> {
	block_on(core_scheduler().replace_object(fd, obj), None)
}

// The dup system call allocates a new file descriptor that refers
// to the same open file description as the descriptor oldfd. The new
// file descriptor number is guaranteed to be the lowest-numbered
// file descriptor that was unused in the calling process.
pub(crate) fn dup_object(fd: FileDescriptor) -> Result<FileDescriptor, IoError> {
	block_on(core_scheduler().dup_object(fd), None)
}

pub(crate) fn remove_object(fd: FileDescriptor) -> Result<Arc<dyn ObjectInterface>, IoError> {
	block_on(core_scheduler().remove_object(fd), None)
}
