use uhyve_interface::parameters::WriteParams;
use uhyve_interface::{GuestVirtAddr, Hypercall};

use crate::fd::{
	AccessPermission, FileAttr, ObjectInterface, PollEvent, STDERR_FILENO, STDOUT_FILENO,
};
use crate::io;
use crate::uhyve::uhyve_hypercall;

pub struct UhyveStdin;

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

impl ObjectInterface for UhyveStdout {
	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		let available = PollEvent::POLLOUT | PollEvent::POLLWRNORM | PollEvent::POLLWRBAND;
		Ok(event & available)
	}

	async fn write(&self, buf: &[u8]) -> io::Result<usize> {
		let write_params = WriteParams {
			fd: STDOUT_FILENO,
			buf: GuestVirtAddr::from_ptr(buf.as_ptr()),
			len: buf.len(),
		};
		uhyve_hypercall(Hypercall::FileWrite(&write_params));

		Ok(write_params.len)
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

impl ObjectInterface for UhyveStderr {
	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		let available = PollEvent::POLLOUT | PollEvent::POLLWRNORM | PollEvent::POLLWRBAND;
		Ok(event & available)
	}

	async fn write(&self, buf: &[u8]) -> io::Result<usize> {
		let write_params = WriteParams {
			fd: STDERR_FILENO,
			buf: GuestVirtAddr::from_ptr(buf.as_ptr()),
			len: buf.len(),
		};
		uhyve_hypercall(Hypercall::FileWrite(&write_params));

		Ok(write_params.len)
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
