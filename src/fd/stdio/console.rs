use core::future;
use core::task::Poll;

use embedded_io::{Read, ReadReady, Write};

use crate::console::{CONSOLE, CONSOLE_WAKER};
use crate::fd::{AccessPermission, FileAttr, ObjectInterface, PollEvent};
use crate::io;

pub struct ConsoleStdin;

impl ObjectInterface for ConsoleStdin {
	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		future::poll_fn(|cx| {
			let readable = PollEvent::POLLIN | PollEvent::POLLRDNORM | PollEvent::POLLRDBAND;
			let (available, requires_polling) = {
				let mut console = CONSOLE.lock();
				(console.read_ready()?, console.requires_input_polling())
			};
			let ready = event
				& if available {
					readable
				} else {
					PollEvent::empty()
				};

			if !ready.is_empty() || !event.intersects(readable) {
				Poll::Ready(Ok(ready))
			} else {
				if requires_polling {
					cx.waker().wake_by_ref();
				} else {
					CONSOLE_WAKER.lock().register(cx.waker());
					if CONSOLE.lock().read_ready()? {
						return Poll::Ready(Ok(event & readable));
					}
				}
				Poll::Pending
			}
		})
		.await
	}

	async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
		future::poll_fn(|cx| {
			let (read_bytes, requires_polling) = {
				let mut console = CONSOLE.lock();
				(console.read(buf)?, console.requires_input_polling())
			};
			if read_bytes > 0 {
				CONSOLE.lock().write_all(&buf[..read_bytes])?;
				CONSOLE.lock().flush()?;
				Poll::Ready(Ok(read_bytes))
			} else if requires_polling {
				cx.waker().wake_by_ref();
				Poll::Pending
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
