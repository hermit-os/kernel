use alloc::boxed::Box;
use core::future;
use core::task::Poll;

use async_trait::async_trait;
use embedded_io::{Read, ReadReady, Write};
use memory_addresses::VirtAddr;
use uhyve_interface::GuestPhysAddr;
use uhyve_interface::v2::Hypercall;
use uhyve_interface::v2::parameters::WriteParams;

use crate::arch::mm::paging;
use crate::console::{CONSOLE, CONSOLE_WAKER};
use crate::fd::{
	AccessPermission, FileAttr, ObjectInterface, PollEvent, STDERR_FILENO, STDOUT_FILENO,
};
use crate::io;
use crate::syscalls::interfaces::uhyve_hypercall;

pub struct GenericStdin;

#[async_trait]
impl ObjectInterface for GenericStdin {
	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		let available = if CONSOLE.lock().read_ready()? {
			PollEvent::POLLIN | PollEvent::POLLRDNORM | PollEvent::POLLRDBAND
		} else {
			PollEvent::empty()
		};

		Ok(event & available)
	}

	async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
		future::poll_fn(|cx| {
			let read_bytes = CONSOLE.lock().read(buf)?;
			if read_bytes > 0 {
				CONSOLE.lock().write_all(&buf[..read_bytes])?;
				CONSOLE.lock().flush()?;
				Poll::Ready(Ok(read_bytes))
			} else {
				CONSOLE_WAKER.lock().register(cx.waker());
				Poll::Pending
			}
		})
		.await
	}

	async fn isatty(&self) -> io::Result<bool> {
		Ok(true)
	}

	async fn fstat(&self) -> io::Result<FileAttr> {
		let attr = FileAttr {
			st_mode: AccessPermission::S_IFCHR,
			..Default::default()
		};
		Ok(attr)
	}
}

impl GenericStdin {
	pub const fn new() -> Self {
		Self {}
	}
}

pub struct GenericStdout;

#[async_trait]
impl ObjectInterface for GenericStdout {
	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		let available = PollEvent::POLLOUT | PollEvent::POLLWRNORM | PollEvent::POLLWRBAND;
		Ok(event & available)
	}

	async fn write(&self, buf: &[u8]) -> io::Result<usize> {
		CONSOLE.lock().write(buf)
	}

	async fn isatty(&self) -> io::Result<bool> {
		Ok(true)
	}

	async fn fstat(&self) -> io::Result<FileAttr> {
		let attr = FileAttr {
			st_mode: AccessPermission::S_IFCHR,
			..Default::default()
		};
		Ok(attr)
	}
}

impl GenericStdout {
	pub const fn new() -> Self {
		Self {}
	}
}

pub struct GenericStderr;

#[async_trait]
impl ObjectInterface for GenericStderr {
	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		let available = PollEvent::POLLOUT | PollEvent::POLLWRNORM | PollEvent::POLLWRBAND;
		Ok(event & available)
	}

	async fn write(&self, buf: &[u8]) -> io::Result<usize> {
		CONSOLE.lock().write(buf)
	}

	async fn isatty(&self) -> io::Result<bool> {
		Ok(true)
	}

	async fn fstat(&self) -> io::Result<FileAttr> {
		let attr = FileAttr {
			st_mode: AccessPermission::S_IFCHR,
			..Default::default()
		};
		Ok(attr)
	}
}

impl GenericStderr {
	pub const fn new() -> Self {
		Self {}
	}
}

pub struct UhyveStdin;

#[async_trait]
impl ObjectInterface for UhyveStdin {
	async fn isatty(&self) -> io::Result<bool> {
		Ok(true)
	}

	async fn fstat(&self) -> io::Result<FileAttr> {
		let attr = FileAttr {
			st_mode: AccessPermission::S_IFCHR,
			..Default::default()
		};
		Ok(attr)
	}
}

impl UhyveStdin {
	pub const fn new() -> Self {
		Self {}
	}
}

pub struct UhyveStdout;

#[async_trait]
impl ObjectInterface for UhyveStdout {
	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		let available = PollEvent::POLLOUT | PollEvent::POLLWRNORM | PollEvent::POLLWRBAND;
		Ok(event & available)
	}

	async fn write(&self, buf: &[u8]) -> io::Result<usize> {
		let mut write_params = WriteParams {
			fd: STDOUT_FILENO,
			buf: GuestPhysAddr::new(
				paging::virtual_to_physical(VirtAddr::from_ptr(buf.as_ptr()))
					.unwrap()
					.as_u64(),
			),
			len: buf.len().try_into().unwrap(),
			ret: 0i64,
		};
		uhyve_hypercall(Hypercall::FileWrite(&mut write_params));
		// Assumption: Uhyve will write to stdout and manually override the ret.
		debug_assert_eq!(write_params.len, write_params.ret as u64);
		Ok(write_params.ret.try_into().unwrap())
	}

	async fn isatty(&self) -> io::Result<bool> {
		Ok(true)
	}

	async fn fstat(&self) -> io::Result<FileAttr> {
		let attr = FileAttr {
			st_mode: AccessPermission::S_IFCHR,
			..Default::default()
		};
		Ok(attr)
	}
}

impl UhyveStdout {
	pub const fn new() -> Self {
		Self {}
	}
}

pub struct UhyveStderr;

#[async_trait]
impl ObjectInterface for UhyveStderr {
	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		let available = PollEvent::POLLOUT | PollEvent::POLLWRNORM | PollEvent::POLLWRBAND;
		Ok(event & available)
	}

	async fn write(&self, buf: &[u8]) -> io::Result<usize> {
		let mut write_params = WriteParams {
			fd: STDERR_FILENO,
			buf: GuestPhysAddr::new(
				paging::virtual_to_physical(VirtAddr::from_ptr(buf.as_ptr()))
					.unwrap()
					.as_u64(),
			),
			len: buf.len().try_into().unwrap(),
			ret: 0i64,
		};
		uhyve_hypercall(Hypercall::FileWrite(&mut write_params));
		// Assumption: Uhyve will write to stdout and manually override the ret.
		debug_assert_eq!(write_params.len, write_params.ret as u64);
		Ok(write_params.ret.try_into().unwrap())
	}

	async fn isatty(&self) -> io::Result<bool> {
		Ok(true)
	}

	async fn fstat(&self) -> io::Result<FileAttr> {
		let attr = FileAttr {
			st_mode: AccessPermission::S_IFCHR,
			..Default::default()
		};
		Ok(attr)
	}
}

impl UhyveStderr {
	pub const fn new() -> Self {
		Self {}
	}
}
