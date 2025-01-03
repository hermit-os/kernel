use alloc::boxed::Box;
use core::future;
use core::task::Poll;

use async_trait::async_trait;
use uhyve_interface::parameters::WriteParams;
use uhyve_interface::{GuestVirtAddr, Hypercall};
use zerocopy::IntoBytes;

use crate::console::CONSOLE;
use crate::fd::{ObjectInterface, PollEvent, STDERR_FILENO, STDOUT_FILENO};
use crate::io;
use crate::syscalls::interfaces::uhyve_hypercall;

#[derive(Debug)]
pub struct GenericStdin;

#[async_trait]
impl ObjectInterface for GenericStdin {
	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		let available = if CONSOLE.lock().is_empty() {
			PollEvent::empty()
		} else {
			PollEvent::POLLIN | PollEvent::POLLRDNORM | PollEvent::POLLRDBAND
		};

		Ok(event & available)
	}

	async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
		future::poll_fn(|cx| {
			let mut read_bytes = 0;
			let mut guard = CONSOLE.lock();

			while let Some(byte) = guard.read() {
				let c = unsafe { char::from_u32_unchecked(byte.into()) };
				guard.write(c.as_bytes());

				buf[read_bytes] = byte;
				read_bytes += 1;

				if read_bytes >= buf.len() {
					return Poll::Ready(Ok(read_bytes));
				}
			}

			if read_bytes > 0 {
				Poll::Ready(Ok(read_bytes))
			} else {
				guard.register_waker(cx.waker());
				Poll::Pending
			}
		})
		.await
	}
}

impl GenericStdin {
	pub const fn new() -> Self {
		Self {}
	}
}

#[derive(Debug)]
pub struct GenericStdout;

#[async_trait]
impl ObjectInterface for GenericStdout {
	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		let available = PollEvent::POLLOUT | PollEvent::POLLWRNORM | PollEvent::POLLWRBAND;
		Ok(event & available)
	}

	async fn write(&self, buf: &[u8]) -> io::Result<usize> {
		CONSOLE.lock().write(buf);
		Ok(buf.len())
	}
}

impl GenericStdout {
	pub const fn new() -> Self {
		Self {}
	}
}

#[derive(Debug)]
pub struct GenericStderr;

#[async_trait]
impl ObjectInterface for GenericStderr {
	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		let available = PollEvent::POLLOUT | PollEvent::POLLWRNORM | PollEvent::POLLWRBAND;
		Ok(event & available)
	}

	async fn write(&self, buf: &[u8]) -> io::Result<usize> {
		CONSOLE.lock().write(buf);
		Ok(buf.len())
	}
}

impl GenericStderr {
	pub const fn new() -> Self {
		Self {}
	}
}

#[derive(Debug)]
pub struct UhyveStdin;

impl ObjectInterface for UhyveStdin {}

impl UhyveStdin {
	pub const fn new() -> Self {
		Self {}
	}
}

#[derive(Debug)]
pub struct UhyveStdout;

#[async_trait]
impl ObjectInterface for UhyveStdout {
	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		let available = PollEvent::POLLOUT | PollEvent::POLLWRNORM | PollEvent::POLLWRBAND;
		Ok(event & available)
	}

	async fn write(&self, buf: &[u8]) -> io::Result<usize> {
		let write_params = WriteParams {
			fd: STDOUT_FILENO,
			buf: GuestVirtAddr::new(buf.as_ptr() as u64),
			len: buf.len(),
		};
		uhyve_hypercall(Hypercall::FileWrite(&write_params));

		Ok(write_params.len)
	}
}

impl UhyveStdout {
	pub const fn new() -> Self {
		Self {}
	}
}

#[derive(Debug)]
pub struct UhyveStderr;

#[async_trait]
impl ObjectInterface for UhyveStderr {
	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		let available = PollEvent::POLLOUT | PollEvent::POLLWRNORM | PollEvent::POLLWRBAND;
		Ok(event & available)
	}

	async fn write(&self, buf: &[u8]) -> io::Result<usize> {
		let write_params = WriteParams {
			fd: STDERR_FILENO,
			buf: GuestVirtAddr::new(buf.as_ptr() as u64),
			len: buf.len(),
		};
		uhyve_hypercall(Hypercall::FileWrite(&write_params));

		Ok(write_params.len)
	}
}

impl UhyveStderr {
	pub const fn new() -> Self {
		Self {}
	}
}
