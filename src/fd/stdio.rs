use core::isize;
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
use core::ptr;

#[cfg(target_arch = "x86_64")]
use x86::io::*;

use crate::arch::mm::{paging, VirtAddr};
use crate::console::CONSOLE;
use crate::fd::{IoError, ObjectInterface, STDERR_FILENO, STDOUT_FILENO};

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
	let ptr = VirtAddr(ptr::from_mut(data).addr() as u64);
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

	let ptr = VirtAddr(ptr::from_mut(data).addr() as u64);
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

#[derive(Debug, Clone)]
pub struct GenericStdin;

impl ObjectInterface for GenericStdin {}

impl GenericStdin {
	pub const fn new() -> Self {
		Self {}
	}
}

#[derive(Debug, Clone)]
pub struct GenericStdout;

impl ObjectInterface for GenericStdout {
	fn write(&self, buf: &[u8]) -> Result<isize, IoError> {
		// stdin/err/out all go to console
		CONSOLE.lock().write_all(buf);

		Ok(buf.len().try_into().unwrap())
	}
}

impl GenericStdout {
	pub const fn new() -> Self {
		Self {}
	}
}

#[derive(Debug, Clone)]
pub struct GenericStderr;

impl ObjectInterface for GenericStderr {
	fn write(&self, buf: &[u8]) -> Result<isize, IoError> {
		// stdin/err/out all go to console
		CONSOLE.lock().write_all(buf);

		Ok(buf.len().try_into().unwrap())
	}
}

impl GenericStderr {
	pub const fn new() -> Self {
		Self {}
	}
}

#[derive(Debug, Clone)]
pub struct UhyveStdin;

impl ObjectInterface for UhyveStdin {}

impl UhyveStdin {
	pub const fn new() -> Self {
		Self {}
	}
}

#[derive(Debug, Clone)]
pub struct UhyveStdout;

impl ObjectInterface for UhyveStdout {
	fn write(&self, buf: &[u8]) -> Result<isize, IoError> {
		let mut syswrite = SysWrite::new(STDOUT_FILENO, buf.as_ptr(), buf.len());
		uhyve_send(UHYVE_PORT_WRITE, &mut syswrite);

		Ok(syswrite.len as isize)
	}
}

impl UhyveStdout {
	pub const fn new() -> Self {
		Self {}
	}
}

#[derive(Debug, Clone)]
pub struct UhyveStderr;

impl ObjectInterface for UhyveStderr {
	fn write(&self, buf: &[u8]) -> Result<isize, IoError> {
		let mut syswrite = SysWrite::new(STDERR_FILENO, buf.as_ptr(), buf.len());
		uhyve_send(UHYVE_PORT_WRITE, &mut syswrite);

		Ok(syswrite.len as isize)
	}
}

impl UhyveStderr {
	pub const fn new() -> Self {
		Self {}
	}
}
