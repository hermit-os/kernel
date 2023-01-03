use alloc::boxed::Box;
use core::{isize, slice, str};

use crate::fd::{
	uhyve_send, ObjectInterface, SysClose, SysLseek, SysRead, SysWrite, UHYVE_PORT_CLOSE,
	UHYVE_PORT_LSEEK, UHYVE_PORT_READ, UHYVE_PORT_WRITE,
};
use crate::syscalls::fs::{self, PosixFile, SeekWhence};

const SEEK_SET: i32 = 0;
const SEEK_CUR: i32 = 1;
const SEEK_END: i32 = 2;

impl TryFrom<i32> for SeekWhence {
	type Error = &'static str;

	fn try_from(value: i32) -> Result<Self, Self::Error> {
		match value {
			SEEK_CUR => Ok(SeekWhence::Cur),
			SEEK_SET => Ok(SeekWhence::Set),
			SEEK_END => Ok(SeekWhence::End),
			_ => Err("Got invalid seek whence parameter!"),
		}
	}
}

#[derive(Debug, Clone)]
pub struct UhyveFile(i32);

impl UhyveFile {
	pub fn new(fd: i32) -> Self {
		Self(fd)
	}
}

impl ObjectInterface for UhyveFile {
	fn write(&self, buf: *const u8, len: usize) -> isize {
		let mut syswrite = SysWrite::new(self.0, buf, len);
		uhyve_send(UHYVE_PORT_WRITE, &mut syswrite);

		syswrite.len as isize
	}

	fn read(&self, buf: *mut u8, len: usize) -> isize {
		let mut sysread = SysRead::new(self.0, buf, len);
		uhyve_send(UHYVE_PORT_READ, &mut sysread);

		sysread.ret
	}

	fn lseek(&self, offset: isize, whence: i32) -> isize {
		let mut syslseek = SysLseek::new(self.0, offset, whence);
		uhyve_send(UHYVE_PORT_LSEEK, &mut syslseek);

		syslseek.offset
	}
}

impl Drop for UhyveFile {
	fn drop(&mut self) {
		let mut sysclose = SysClose::new(self.0);
		uhyve_send(UHYVE_PORT_CLOSE, &mut sysclose);
	}
}

#[derive(Debug, Clone)]
pub struct GenericFile(u64);

impl GenericFile {
	pub fn new(fd: u64) -> Self {
		Self(fd)
	}
}

impl ObjectInterface for GenericFile {
	fn write(&self, buf: *const u8, len: usize) -> isize {
		assert!(len <= isize::MAX as usize);
		let buf = unsafe { slice::from_raw_parts(buf, len) };

		// Normal file
		let mut written_bytes = 0;
		let mut fs = fs::FILESYSTEM.lock();
		fs.fd_op(self.0, |file: &mut Box<dyn PosixFile + Send>| {
			written_bytes = file.write(buf).unwrap(); // TODO: might fail
		});
		debug!("Write done! {}", written_bytes);
		written_bytes as isize
	}

	fn read(&self, buf: *mut u8, len: usize) -> isize {
		debug!("Read! {}, {}", self.0, len);

		let mut fs = fs::FILESYSTEM.lock();
		let mut read_bytes = 0;
		fs.fd_op(self.0, |file: &mut Box<dyn PosixFile + Send>| {
			let dat = file.read(len as u32).unwrap(); // TODO: might fail

			read_bytes = dat.len();
			unsafe {
				core::slice::from_raw_parts_mut(buf, read_bytes).copy_from_slice(&dat);
			}
		});

		read_bytes as isize
	}

	fn lseek(&self, offset: isize, whence: i32) -> isize {
		debug!("lseek! {}, {}, {}", self.0, offset, whence);

		let mut fs = fs::FILESYSTEM.lock();
		let mut ret = 0;
		fs.fd_op(self.0, |file: &mut Box<dyn PosixFile + Send>| {
			ret = file.lseek(offset, whence.try_into().unwrap()).unwrap(); // TODO: might fail
		});

		ret as isize
	}
}

impl Drop for GenericFile {
	fn drop(&mut self) {
		let mut fs = fs::FILESYSTEM.lock();
		fs.close(self.0);
	}
}
