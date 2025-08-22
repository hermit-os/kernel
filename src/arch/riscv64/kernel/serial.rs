use core::mem::MaybeUninit;

use crate::io;

pub(crate) struct SerialDevice;

impl SerialDevice {
	pub fn new() -> Self {
		Self {}
	}

	pub fn write(&self, buf: &[u8]) {
		for byte in buf {
			sbi_rt::console_write_byte(*byte);
		}
	}

	pub fn read(&self, _buf: &mut [MaybeUninit<u8>]) -> io::Result<usize> {
		Ok(0)
	}

	pub fn can_read(&self) -> bool {
		false
	}
}
