use alloc::boxed::Box;
use core::future;
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
use core::ptr;
use core::task::Poll;

use async_trait::async_trait;
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
use memory_addresses::VirtAddr;
#[cfg(target_arch = "x86_64")]
use x86::io::*;
use zerocopy::IntoBytes;

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
use crate::arch::mm::paging;
use crate::console::CONSOLE;
use crate::fd::{ObjectInterface, PollEvent, STDERR_FILENO, STDOUT_FILENO};
use crate::io;

const UHYVE_PORT_WRITE: u16 = 0x400;

#[repr(C, packed)]
struct SysWrite {
	fd: i32,
	buf: *const u8,
	len: usize,
}

impl SysWrite {
	pub fn new(fd: i32, buf: *const u8, len: usize) -> SysWrite {
		SysWrite { fd, buf, len }
	}
}

/// forward a request to the hypervisor uhyve
#[inline]
#[cfg(target_arch = "x86_64")]
fn uhyve_send<T>(port: u16, data: &mut T) {
	let ptr = VirtAddr::from_ptr(ptr::from_mut(data));
	let physical_address = paging::virtual_to_physical(ptr).unwrap();

	unsafe {
		outl(port, physical_address.as_u64() as u32);
	}
}

/// forward a request to the hypervisor uhyve
#[inline]
#[cfg(target_arch = "aarch64")]
fn uhyve_send<T>(port: u16, data: &mut T) {
	use core::arch::asm;

	let ptr = VirtAddr::from_ptr(ptr::from_mut(data));
	let physical_address = paging::virtual_to_physical(ptr).unwrap();

	unsafe {
		asm!(
			"str x8, [{port}]",
			port = in(reg) u64::from(port),
			in("x8") physical_address.as_u64(),
			options(nostack),
		);
	}
}

/// forward a request to the hypervisor uhyve
#[inline]
#[cfg(target_arch = "riscv64")]
fn uhyve_send<T>(_port: u16, _data: &mut T) {
	todo!()
}

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
		let mut syswrite = SysWrite::new(STDOUT_FILENO, buf.as_ptr(), buf.len());
		uhyve_send(UHYVE_PORT_WRITE, &mut syswrite);

		Ok(syswrite.len)
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
		let mut syswrite = SysWrite::new(STDERR_FILENO, buf.as_ptr(), buf.len());
		uhyve_send(UHYVE_PORT_WRITE, &mut syswrite);

		Ok(syswrite.len)
	}
}

impl UhyveStderr {
	pub const fn new() -> Self {
		Self {}
	}
}
