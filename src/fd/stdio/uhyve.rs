use uhyve_interface::v3::Hypercall;
use uhyve_interface::v3::util::RawResult;
use uhyve_interface::v3::parameters::{ReadParams, WriteParams};

use crate::fd::{
	AccessPermission, FileAttr, ObjectInterface, PollEvent, STDERR_FILENO, STDIN_FILENO,
	STDOUT_FILENO,
};
use crate::io;
use crate::uhyve::{as_mut_sized_guest_buf, as_sized_guest_buf, uhyve_hypercall};

pub struct UhyveStdin;

impl ObjectInterface for UhyveStdin {
	async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
		let mut read_params = ReadParams {
			fd: STDIN_FILENO,
			buf: as_mut_sized_guest_buf(buf),
			ret: RawResult::default(),
		};
		uhyve_hypercall(Hypercall::FileRead(&mut read_params));
		let n = read_params.ret.decode().unwrap()?;
		Ok(n.try_into().unwrap())
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
		let mut write_params = WriteParams {
			fd: STDOUT_FILENO,
			buf: as_sized_guest_buf(buf),
			ret: RawResult::default(),
		};
		uhyve_hypercall(Hypercall::FileWrite(&mut write_params));
		let n = write_params.ret.decode().unwrap()?;
		Ok(n.try_into().unwrap())
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
		let mut write_params = WriteParams {
			fd: STDERR_FILENO,
			buf: as_sized_guest_buf(buf),
			ret: RawResult::default(),
		};

		// fd refers to a regular file
		uhyve_hypercall(Hypercall::FileWrite(&mut write_params));
		let n = write_params.ret.decode().unwrap()?;
		Ok(n.try_into().unwrap())
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
