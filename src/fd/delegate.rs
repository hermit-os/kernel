#[cfg(any(feature = "net", feature = "virtio-vsock"))]
use alloc::sync::Arc;
use core::mem::MaybeUninit;

use delegate::delegate;

use crate::fd::eventfd::EventFd;
#[cfg(feature = "tcp")]
use crate::fd::socket::tcp;
#[cfg(feature = "udp")]
use crate::fd::socket::udp;
#[cfg(feature = "virtio-vsock")]
use crate::fd::socket::vsock;
use crate::fd::stdio::{
	GenericStderr, GenericStdin, GenericStdout, UhyveStderr, UhyveStdin, UhyveStdout,
};
use crate::fd::{AccessPermission, ObjectInterface, PollEvent, StatusFlags};
#[cfg(any(feature = "net", feature = "virtio-vsock"))]
use crate::fd::{Endpoint, ListenEndpoint, SocketOption};
#[cfg(feature = "virtio-fs")]
use crate::fs::fuse::{FuseDirectoryHandle, FuseFileHandle};
use crate::fs::mem::{MemDirectoryInterface, RamFileInterface, RomFileInterface};
use crate::fs::uhyve::UhyveFileHandle;
use crate::fs::{DirectoryReader, FileAttr, SeekWhence};
use crate::io;

pub(crate) enum Fd {
	GenericStdin(GenericStdin),
	GenericStdout(GenericStdout),
	GenericStderr(GenericStderr),
	UhyveStdin(UhyveStdin),
	UhyveStdout(UhyveStdout),
	UhyveStderr(UhyveStderr),
	EventFd(EventFd),
	#[cfg(feature = "tcp")]
	TcpSocket(tcp::Socket),
	#[cfg(feature = "udp")]
	UdpSocket(udp::Socket),
	#[cfg(feature = "virtio-vsock")]
	VsockNullSocket(vsock::NullSocket),
	#[cfg(feature = "virtio-vsock")]
	VsockSocket(vsock::Socket),
	#[cfg(feature = "virtio-fs")]
	FuseFileHandle(FuseFileHandle),
	#[cfg(feature = "virtio-fs")]
	FuseDirectoryHandle(FuseDirectoryHandle),
	RomFileInterface(RomFileInterface),
	RamFileInterface(RamFileInterface),
	MemDirectoryInterface(MemDirectoryInterface),
	DirectoryReader(DirectoryReader),
	UhyveFileHandle(UhyveFileHandle),
}

macro_rules! fd_from {
	() => {};
	(
		$(#[$meta:meta])*
		$ident:ident($ty:ty),
		$($rest:tt)*
	) => {
		$(#[$meta])*
		impl From<$ty> for Fd {
			fn from(value: $ty) -> Self {
				Self::$ident(value)
			}
		}

		fd_from!($($rest)*);
	};
}

fd_from! {
	GenericStdin(GenericStdin),
	GenericStdout(GenericStdout),
	GenericStderr(GenericStderr),
	UhyveStdin(UhyveStdin),
	UhyveStdout(UhyveStdout),
	UhyveStderr(UhyveStderr),
	EventFd(EventFd),
	#[cfg(feature = "tcp")]
	TcpSocket(tcp::Socket),
	#[cfg(feature = "udp")]
	UdpSocket(udp::Socket),
	#[cfg(feature = "virtio-vsock")]
	VsockNullSocket(vsock::NullSocket),
	#[cfg(feature = "virtio-vsock")]
	VsockSocket(vsock::Socket),
	#[cfg(feature = "virtio-fs")]
	FuseFileHandle(FuseFileHandle),
	#[cfg(feature = "virtio-fs")]
	FuseDirectoryHandle(FuseDirectoryHandle),
	RomFileInterface(RomFileInterface),
	RamFileInterface(RamFileInterface),
	MemDirectoryInterface(MemDirectoryInterface),
	DirectoryReader(DirectoryReader),
	UhyveFileHandle(UhyveFileHandle),
}

impl ObjectInterface for Fd {
	delegate! {
		to match self {
			Self::GenericStdin(fd) => fd,
			Self::GenericStdout(fd) => fd,
			Self::GenericStderr(fd) => fd,
			Self::UhyveStdin(fd) => fd,
			Self::UhyveStdout(fd) => fd,
			Self::UhyveStderr(fd) => fd,
			Self::EventFd(fd) => fd,
			#[cfg(feature = "tcp")]
			Self::TcpSocket(fd) => fd,
			#[cfg(feature = "udp")]
			Self::UdpSocket(fd) => fd,
			#[cfg(feature = "virtio-vsock")]
			Self::VsockNullSocket(fd) => fd,
			#[cfg(feature = "virtio-vsock")]
			Self::VsockSocket(fd) => fd,
			#[cfg(feature = "virtio-fs")]
			Self::FuseFileHandle(fd) => fd,
			#[cfg(feature = "virtio-fs")]
			Self::FuseDirectoryHandle(fd) => fd,
			Self::RomFileInterface(fd) => fd,
			Self::RamFileInterface(fd) => fd,
			Self::MemDirectoryInterface(fd) => fd,
			Self::DirectoryReader(fd) => fd,
			Self::UhyveFileHandle(fd) => fd,
		} {
			async fn poll(&self, event: PollEvent) -> io::Result<PollEvent>;
			async fn read(&self, buf: &mut [u8]) -> io::Result<usize>;
			async fn write(&self, buf: &[u8]) -> io::Result<usize>;
			async fn lseek(&self, offset: isize, whence: SeekWhence) -> io::Result<isize>;
			async fn fstat(&self) -> io::Result<FileAttr>;
			async fn getdents(&self, buf: &mut [MaybeUninit<u8>]) -> io::Result<usize>;
			#[cfg(any(feature = "net", feature = "virtio-vsock"))]
			async fn accept(&mut self) -> io::Result<(Arc<async_lock::RwLock<Fd>>, Endpoint)>;
			#[cfg(any(feature = "net", feature = "virtio-vsock"))]
			async fn connect(&mut self, endpoint: Endpoint) -> io::Result<()>;
			#[cfg(any(feature = "net", feature = "virtio-vsock"))]
			async fn bind(&mut self, _name: ListenEndpoint) -> io::Result<()>;
			#[cfg(any(feature = "net", feature = "virtio-vsock"))]
			async fn listen(&mut self, _backlog: i32) -> io::Result<()>;
			#[cfg(any(feature = "net", feature = "virtio-vsock"))]
			async fn setsockopt(&self, _opt: SocketOption, _optval: bool) -> io::Result<()>;
			#[cfg(any(feature = "net", feature = "virtio-vsock"))]
			async fn getsockopt(&self, _opt: SocketOption) -> io::Result<bool>;
			#[cfg(any(feature = "net", feature = "virtio-vsock"))]
			async fn getsockname(&self) -> io::Result<Option<Endpoint>>;
			#[cfg(any(feature = "net", feature = "virtio-vsock"))]
			#[allow(dead_code)]
			async fn getpeername(&self) -> io::Result<Option<Endpoint>>;
			#[cfg(any(feature = "net", feature = "virtio-vsock"))]
			async fn recvfrom(&self, _buffer: &mut [MaybeUninit<u8>]) -> io::Result<(usize, Endpoint)>;
			#[cfg(any(feature = "net", feature = "virtio-vsock"))]
			async fn sendto(&self, _buffer: &[u8], _endpoint: Endpoint) -> io::Result<usize>;
			#[cfg(any(feature = "net", feature = "virtio-vsock"))]
			async fn shutdown(&self, _how: i32) -> io::Result<()>;
			async fn status_flags(&self) -> io::Result<StatusFlags>;
			async fn set_status_flags(&mut self, _status_flags: StatusFlags) -> io::Result<()>;
			async fn truncate(&self, _size: usize) -> io::Result<()>;
			async fn chmod(&self, _access_permission: AccessPermission) -> io::Result<()>;
			async fn isatty(&self) -> io::Result<bool>;
		}
	}
}
