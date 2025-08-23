use embedded_io::{ErrorType, Write};

use crate::errno::Errno;
use crate::io;

pub(crate) struct SerialDevice;

impl SerialDevice {
	pub fn new() -> Self {
		Self {}
	}

	pub fn read(&self, _buf: &mut [u8]) -> io::Result<usize> {
		Ok(0)
	}

	pub fn can_read(&self) -> bool {
		false
	}
}

impl ErrorType for SerialDevice {
	type Error = Errno;
}

impl Write for SerialDevice {
	fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
		for byte in buf {
			sbi_rt::console_write_byte(*byte);
		}

		Ok(buf.len())
	}

	fn flush(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}
}
