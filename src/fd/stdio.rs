use core::{isize, slice};

use crate::console::CONSOLE;
use crate::fd::{
	uhyve_send, ObjectInterface, SysWrite, STDERR_FILENO, STDOUT_FILENO, UHYVE_PORT_WRITE,
};

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
	fn write(&self, buf: *const u8, len: usize) -> isize {
		assert!(len <= isize::MAX as usize);
		let buf = unsafe { slice::from_raw_parts(buf, len) };

		// stdin/err/out all go to console
		CONSOLE.lock().write_all(buf);

		len as isize
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
	fn write(&self, buf: *const u8, len: usize) -> isize {
		assert!(len <= isize::MAX as usize);
		let buf = unsafe { slice::from_raw_parts(buf, len) };

		// stdin/err/out all go to console
		CONSOLE.lock().write_all(buf);

		len as isize
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
	fn write(&self, buf: *const u8, len: usize) -> isize {
		let mut syswrite = SysWrite::new(STDOUT_FILENO, buf, len);
		uhyve_send(UHYVE_PORT_WRITE, &mut syswrite);

		syswrite.len as isize
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
	fn write(&self, buf: *const u8, len: usize) -> isize {
		let mut syswrite = SysWrite::new(STDERR_FILENO, buf, len);
		uhyve_send(UHYVE_PORT_WRITE, &mut syswrite);

		syswrite.len as isize
	}
}

impl UhyveStderr {
	pub const fn new() -> Self {
		Self {}
	}
}
