use memory_addresses::VirtAddr;
use uhyve_interface::GuestPhysAddr;
use uhyve_interface::v2::Hypercall;
use uhyve_interface::v2::parameters::{ReadParams, WriteParams};

use crate::arch::mm::paging;
use crate::errno::Errno;
use crate::fd::{
	AccessPermission, FileAttr, ObjectInterface, PollEvent, STDERR_FILENO, STDIN_FILENO,
	STDOUT_FILENO,
};
use crate::io;
use crate::uhyve::uhyve_hypercall;

pub struct UhyveStdin;

impl ObjectInterface for UhyveStdin {
	async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
		let mut read_params = ReadParams {
			fd: STDIN_FILENO,
			buf: GuestPhysAddr::new(
				paging::virtual_to_physical(VirtAddr::from_ptr(buf.as_mut_ptr()))
					.unwrap()
					.as_u64(),
			),
			len: buf.len().try_into().unwrap(),
			ret: 0i64,
		};
		uhyve_hypercall(Hypercall::FileRead(&mut read_params));
		match read_params.ret {
			ret if ret >= 0 => Ok(ret.try_into().unwrap()),
			_ => Err((read_params.ret as i32).abs().try_into().unwrap()),
		}
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
			buf: GuestPhysAddr::new(
				paging::virtual_to_physical(VirtAddr::from_ptr(buf.as_ptr()))
					.unwrap()
					.as_u64(),
			),
			len: buf.len().try_into().unwrap(),
			ret: 0i64,
		};
		// fd refers to a regular file
		uhyve_hypercall(Hypercall::FileWrite(&mut write_params));
		match write_params.ret {
			// Assumption: fd is a regular file, a zero is only valid if the len
			// (aka. "count") is also zero. Otherwise, however, we assume that something
			// is wrong in Hermit<>Uhyve communication.
			ret if ret > 0 || (ret == 0 && write_params.len == 0) => Ok(ret.try_into().unwrap()),
			errno if errno < 0 => Err((errno as i32).abs().try_into().unwrap()),
			_ => {
				debug!("Uhyve write hypercall yielded a zero.");
				Err(Errno::Inval)
			}
		}
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
			buf: GuestPhysAddr::new(
				paging::virtual_to_physical(VirtAddr::from_ptr(buf.as_ptr()))
					.unwrap()
					.as_u64(),
			),
			len: buf.len().try_into().unwrap(),
			ret: 0i64,
		};

		// fd refers to a regular file
		uhyve_hypercall(Hypercall::FileWrite(&mut write_params));
		match write_params.ret {
			// Assumption: fd is a regular file, a zero is only valid if the len
			// (aka. "count") is also zero. Otherwise, however, we assume that something
			// is wrong in Hermit<>Uhyve communication.
			ret if ret > 0 || (ret == 0 && write_params.len == 0) => Ok(ret.try_into().unwrap()),
			errno if errno < 0 => Err((errno as i32).abs().try_into().unwrap()),
			_ => {
				debug!("Uhyve write hypercall yielded a zero.");
				Err(Errno::Inval)
			}
		}
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
