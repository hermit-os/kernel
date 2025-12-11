use alloc::boxed::Box;
use alloc::sync::Arc;
use core::future::{self, Future};
use core::mem::MaybeUninit;
use core::task::Poll::{Pending, Ready};
use core::time::Duration;

use async_trait::async_trait;
#[cfg(feature = "net")]
use smoltcp::wire::{IpEndpoint, IpListenEndpoint};

use crate::arch::kernel::core_local::core_scheduler;
use crate::errno::Errno;
use crate::executor::block_on;
use crate::fs::{FileAttr, SeekWhence};
use crate::io;

mod eventfd;
#[cfg(any(feature = "net", feature = "virtio-vsock"))]
pub(crate) mod socket;
pub(crate) mod stdio;

pub(crate) const STDIN_FILENO: FileDescriptor = 0;
pub(crate) const STDOUT_FILENO: FileDescriptor = 1;
pub(crate) const STDERR_FILENO: FileDescriptor = 2;

#[cfg(any(feature = "net", feature = "virtio-vsock"))]
#[derive(Debug)]
pub(crate) enum Endpoint {
	#[cfg(feature = "net")]
	Ip(IpEndpoint),
	#[cfg(feature = "virtio-vsock")]
	Vsock(socket::vsock::VsockEndpoint),
}

#[cfg(any(feature = "net", feature = "virtio-vsock"))]
#[derive(Debug)]
pub(crate) enum ListenEndpoint {
	#[cfg(feature = "net")]
	Ip(IpListenEndpoint),
	#[cfg(feature = "virtio-vsock")]
	Vsock(socket::vsock::VsockListenEndpoint),
}

#[allow(dead_code)]
#[derive(Debug, PartialEq)]
pub(crate) enum SocketOption {
	TcpNoDelay,
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
		const O_APPEND = StatusFlags::O_APPEND.bits();
		const O_NONBLOCK = StatusFlags::O_NONBLOCK.bits();
		const O_DIRECT = 0o40000;
		const O_DIRECTORY = 0o200_000;
		/// `O_CLOEXEC` has no functionality in Hermit and will be silently ignored
		const O_CLOEXEC = 0o2_000_000;
	}
}

bitflags! {
	/// Options for checking file permissions or existence
	#[derive(Debug, Copy, Clone, Default, Eq, PartialEq)]
	pub struct AccessOption: i32 {
		/// Test for read permission
		const R_OK = 4;
		/// Test for write permission
		const W_OK = 2;
		/// Test for execution permission
		const X_OK = 1;
		/// Test for existence
		const F_OK = 0;
	}
}

impl AccessOption {
	/// Verifies if the current access options are all valid for the provided file access permissions
	pub fn can_access(&self, access_permissions: AccessPermission) -> bool {
		if self.contains(AccessOption::R_OK)
			&& !access_permissions.contains(AccessPermission::S_IRUSR)
			&& !access_permissions.contains(AccessPermission::S_IRGRP)
			&& !access_permissions.contains(AccessPermission::S_IROTH)
		{
			return false;
		}

		if self.contains(AccessOption::W_OK)
			&& !access_permissions.contains(AccessPermission::S_IWUSR)
			&& !access_permissions.contains(AccessPermission::S_IWGRP)
			&& !access_permissions.contains(AccessPermission::S_IWOTH)
		{
			return false;
		}

		if self.contains(AccessOption::X_OK)
			&& !access_permissions.contains(AccessPermission::S_IXUSR)
			&& !access_permissions.contains(AccessPermission::S_IXGRP)
			&& !access_permissions.contains(AccessPermission::S_IXOTH)
		{
			return false;
		}

		true
	}
}

bitflags! {
	/// File status flags.
	#[derive(Debug, Copy, Clone, Default)]
	pub struct StatusFlags: i32 {
		const O_APPEND = 0o2000;
		const O_NONBLOCK = 0o4000;
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
		const S_IFMT = 0o170_000;
		const S_IFSOCK = 0o140_000;
		const S_IFLNK = 0o120_000;
		const S_IFREG = 0o100_000;
		const S_IFBLK = 0o060_000;
		const S_IFDIR = 0o040_000;
		const S_IFCHR = 0o020_000;
		const S_IFIFO = 0o010_000;
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
pub(crate) trait ObjectInterface: Sync + Send {
	/// check if an IO event is possible
	async fn poll(&self, _event: PollEvent) -> io::Result<PollEvent> {
		Ok(PollEvent::empty())
	}

	/// `async_read` attempts to read `len` bytes from the object references
	/// by the descriptor
	async fn read(&self, _buf: &mut [u8]) -> io::Result<usize> {
		Err(Errno::Nosys)
	}

	/// `async_write` attempts to write `len` bytes to the object references
	/// by the descriptor
	async fn write(&self, _buf: &[u8]) -> io::Result<usize> {
		Err(Errno::Nosys)
	}

	/// `lseek` function repositions the offset of the file descriptor fildes
	async fn lseek(&self, _offset: isize, _whence: SeekWhence) -> io::Result<isize> {
		Err(Errno::Inval)
	}

	/// `fstat`
	async fn fstat(&self) -> io::Result<FileAttr> {
		Err(Errno::Inval)
	}

	/// `getdents` fills the given buffer `_buf` with [`Dirent64`](crate::syscalls::Dirent64)
	/// formatted entries of a directory, imitating the Linux `getdents64` syscall.
	/// On success, the number of bytes read is returned.  On end of directory, 0 is returned.  On error, -1 is returned
	async fn getdents(&self, _buf: &mut [MaybeUninit<u8>]) -> io::Result<usize> {
		Err(Errno::Inval)
	}

	/// `accept` a connection on a socket
	#[cfg(any(feature = "net", feature = "virtio-vsock"))]
	async fn accept(
		&mut self,
	) -> io::Result<(Arc<async_lock::RwLock<dyn ObjectInterface>>, Endpoint)> {
		Err(Errno::Inval)
	}

	/// initiate a connection on a socket
	#[cfg(any(feature = "net", feature = "virtio-vsock"))]
	async fn connect(&mut self, _endpoint: Endpoint) -> io::Result<()> {
		Err(Errno::Inval)
	}

	/// `bind` a name to a socket
	#[cfg(any(feature = "net", feature = "virtio-vsock"))]
	async fn bind(&mut self, _name: ListenEndpoint) -> io::Result<()> {
		Err(Errno::Inval)
	}

	/// `listen` for connections on a socket
	#[cfg(any(feature = "net", feature = "virtio-vsock"))]
	async fn listen(&mut self, _backlog: i32) -> io::Result<()> {
		Err(Errno::Inval)
	}

	/// `setsockopt` sets options on sockets
	#[cfg(any(feature = "net", feature = "virtio-vsock"))]
	async fn setsockopt(&self, _opt: SocketOption, _optval: bool) -> io::Result<()> {
		Err(Errno::Notsock)
	}

	/// `getsockopt` gets options on sockets
	#[cfg(any(feature = "net", feature = "virtio-vsock"))]
	async fn getsockopt(&self, _opt: SocketOption) -> io::Result<bool> {
		Err(Errno::Notsock)
	}

	/// `getsockname` gets socket name
	#[cfg(any(feature = "net", feature = "virtio-vsock"))]
	async fn getsockname(&self) -> io::Result<Option<Endpoint>> {
		Ok(None)
	}

	/// `getpeername` get address of connected peer
	#[cfg(any(feature = "net", feature = "virtio-vsock"))]
	#[allow(dead_code)]
	async fn getpeername(&self) -> io::Result<Option<Endpoint>> {
		Ok(None)
	}

	/// receive a message from a socket
	#[cfg(any(feature = "net", feature = "virtio-vsock"))]
	async fn recvfrom(&self, _buffer: &mut [MaybeUninit<u8>]) -> io::Result<(usize, Endpoint)> {
		Err(Errno::Nosys)
	}

	/// send a message from a socket
	///
	/// The sendto() function shall send a message.
	/// If the socket is a connectionless-mode socket, the message shall
	/// If a peer address has been prespecified, either the message shall
	/// be sent to the address specified by dest_addr (overriding the pre-specified peer
	/// address).
	#[cfg(any(feature = "net", feature = "virtio-vsock"))]
	async fn sendto(&self, _buffer: &[u8], _endpoint: Endpoint) -> io::Result<usize> {
		Err(Errno::Nosys)
	}

	/// shut down part of a full-duplex connection
	#[cfg(any(feature = "net", feature = "virtio-vsock"))]
	async fn shutdown(&self, _how: i32) -> io::Result<()> {
		Err(Errno::Nosys)
	}

	/// Returns the file status flags.
	async fn status_flags(&self) -> io::Result<StatusFlags> {
		Err(Errno::Nosys)
	}

	/// Sets the file status flags.
	async fn set_status_flags(&mut self, _status_flags: StatusFlags) -> io::Result<()> {
		Err(Errno::Nosys)
	}

	/// Truncates the file
	async fn truncate(&self, _size: usize) -> io::Result<()> {
		Err(Errno::Nosys)
	}

	/// Changes access permissions to the file
	async fn chmod(&self, _access_permission: AccessPermission) -> io::Result<()> {
		Err(Errno::Nosys)
	}

	/// `isatty` returns `true` for a terminal device
	async fn isatty(&self) -> io::Result<bool> {
		Ok(false)
	}
}

pub(crate) fn read(fd: FileDescriptor, buf: &mut [u8]) -> io::Result<usize> {
	let obj = get_object(fd)?;

	if buf.is_empty() {
		return Ok(0);
	}

	block_on(async { obj.read().await.read(buf).await }, None)
}

pub(crate) fn lseek(fd: FileDescriptor, offset: isize, whence: SeekWhence) -> io::Result<isize> {
	let obj = get_object(fd)?;

	block_on(async { obj.read().await.lseek(offset, whence).await }, None)
}

pub(crate) fn chmod(fd: FileDescriptor, mode: AccessPermission) -> io::Result<()> {
	let obj = get_object(fd)?;

	block_on(async { obj.read().await.chmod(mode).await }, None)
}

pub(crate) fn write(fd: FileDescriptor, buf: &[u8]) -> io::Result<usize> {
	let obj = get_object(fd)?;

	if buf.is_empty() {
		return Ok(0);
	}

	block_on(async { obj.read().await.write(buf).await }, None)
}

pub(crate) fn truncate(fd: FileDescriptor, length: usize) -> io::Result<()> {
	let obj = get_object(fd)?;
	block_on(async { obj.read().await.truncate(length).await }, None)
}

async fn poll_fds(fds: &mut [PollFd]) -> io::Result<u64> {
	future::poll_fn(|cx| {
		let mut counter: u64 = 0;

		for i in &mut *fds {
			let fd = i.fd;
			i.revents = PollEvent::empty();
			if let Ok(obj) = core_scheduler().get_object(fd) {
				let mut pinned = core::pin::pin!(async { obj.read().await.poll(i.events).await });
				if let Ready(Ok(e)) = pinned.as_mut().poll(cx)
					&& !e.is_empty()
				{
					counter += 1;
					i.revents = e;
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

/// Wait for some event on a file descriptor.
///
/// The unix-like `poll` waits for one of a set of file descriptors
/// to become ready to perform I/O. The set of file descriptors to be
/// monitored is specified in the `fds` argument, which is an array
/// of structs of `PollFd`.
pub fn poll(fds: &mut [PollFd], timeout: Option<Duration>) -> io::Result<u64> {
	let result = block_on(poll_fds(fds), timeout);
	if let Err(ref e) = result
		&& timeout.is_some()
	{
		// A return value of zero indicates that the system call timed out
		if *e == Errno::Again {
			return Ok(0);
		}
	}

	result
}

pub fn fstat(fd: FileDescriptor) -> io::Result<FileAttr> {
	let obj = get_object(fd)?;
	block_on(async { obj.read().await.fstat().await }, None)
}

/// Wait for some event on a file descriptor.
///
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
pub fn eventfd(initval: u64, flags: EventFlags) -> io::Result<FileDescriptor> {
	let obj = self::eventfd::EventFd::new(initval, flags);

	let fd = core_scheduler().insert_object(Arc::new(async_lock::RwLock::new(obj)))?;

	Ok(fd)
}

pub(crate) fn get_object(
	fd: FileDescriptor,
) -> io::Result<Arc<async_lock::RwLock<dyn ObjectInterface>>> {
	core_scheduler().get_object(fd)
}

pub(crate) fn insert_object(
	obj: Arc<async_lock::RwLock<dyn ObjectInterface>>,
) -> io::Result<FileDescriptor> {
	core_scheduler().insert_object(obj)
}

// The dup system call allocates a new file descriptor that refers
// to the same open file description as the descriptor oldfd. The new
// file descriptor number is guaranteed to be the lowest-numbered
// file descriptor that was unused in the calling process.
pub(crate) fn dup_object(fd: FileDescriptor) -> io::Result<FileDescriptor> {
	core_scheduler().dup_object(fd)
}

pub(crate) fn dup_object2(fd1: FileDescriptor, fd2: FileDescriptor) -> io::Result<FileDescriptor> {
	core_scheduler().dup_object2(fd1, fd2)
}

pub(crate) fn remove_object(
	fd: FileDescriptor,
) -> io::Result<Arc<async_lock::RwLock<dyn ObjectInterface>>> {
	core_scheduler().remove_object(fd)
}

pub(crate) fn isatty(fd: FileDescriptor) -> io::Result<bool> {
	let obj = get_object(fd)?;
	block_on(async { obj.read().await.isatty().await }, None)
}
