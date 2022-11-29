use core::{isize, slice};

use crate::console::CONSOLE;
use crate::fd::{
	uhyve_send, ObjectInterface, SysWrite, STDERR_FILENO, STDOUT_FILENO, UHYVE_PORT_WRITE,
};

#[derive(Debug)]
pub struct GenericStdin;

impl ObjectInterface for GenericStdin {
	fn close(&self) -> i32 {
		0
	}
}

impl GenericStdin {
	pub const fn new() -> Self {
		Self {}
	}
}

#[derive(Debug)]
pub struct GenericStdout;

impl ObjectInterface for GenericStdout {
	fn close(&self) -> i32 {
		0
	}

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

#[derive(Debug)]
pub struct GenericStderr;

impl ObjectInterface for GenericStderr {
	fn close(&self) -> i32 {
		0
	}

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

#[derive(Debug)]
pub struct UhyveStdin;

impl ObjectInterface for UhyveStdin {
	fn close(&self) -> i32 {
		0
	}
}

impl UhyveStdin {
	pub const fn new() -> Self {
		Self {}
	}
}

#[derive(Debug)]
pub struct UhyveStdout;

impl ObjectInterface for UhyveStdout {
	fn close(&self) -> i32 {
		0
	}

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

#[derive(Debug)]
pub struct UhyveStderr;

impl ObjectInterface for UhyveStderr {
	fn close(&self) -> i32 {
		0
	}

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
