use core::future;
use core::task::Poll;

use embedded_io::{Read, ReadReady, Write};

use crate::console::{CONSOLE, CONSOLE_WAKER};
use crate::fd::{AccessPermission, FileAttr, ObjectInterface, PollEvent};
use crate::io;

pub struct ConsoleStdin;

impl ObjectInterface for ConsoleStdin {
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

impl ConsoleStdin {
	pub const fn new() -> Self {
		Self {}
	}
}

pub struct ConsoleStdout;

impl ObjectInterface for ConsoleStdout {
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

impl ConsoleStdout {
	pub const fn new() -> Self {
		Self {}
	}
}

pub struct ConsoleStderr;

impl ObjectInterface for ConsoleStderr {
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

impl ConsoleStderr {
	pub const fn new() -> Self {
		Self {}
	}
}
