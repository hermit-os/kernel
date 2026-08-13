use core::hint;

use embedded_io::{ErrorType, Read, ReadReady, Write};
use sbi_rt::Physical;

use crate::errno::Errno;

const SBI_CONSOLE_BUFFER_SIZE: usize = 256;

#[repr(C, align(4096))]
pub(crate) struct SerialDevice {
	sbi_buffer: [u8; SBI_CONSOLE_BUFFER_SIZE],
	buffered_byte: Option<u8>,
}

impl SerialDevice {
	pub fn new() -> Self {
		Self {
			sbi_buffer: [0; SBI_CONSOLE_BUFFER_SIZE],
			buffered_byte: None,
		}
	}

	fn read_from_console(&mut self, buf: &mut [u8]) -> Result<usize, Errno> {
		let len = buf.len().min(self.sbi_buffer.len());
		if len == 0 {
			return Ok(0);
		}

		// Kernel data is identity-mapped on RISC-V. Using a page-aligned bounce buffer
		// avoids walking the page table before it exists or while it is already locked.
		let physical = Physical::<&mut [u8]>::new(len, self.sbi_buffer.as_mut_ptr().addr(), 0);
		let read = sbi_rt::console_read(physical)
			.into_result()
			.map_err(|_| Errno::Io)?;

		if read > len {
			return Err(Errno::Io);
		}

		buf[..read].copy_from_slice(&self.sbi_buffer[..read]);
		Ok(read)
	}

	fn write_to_console(&mut self, buf: &[u8]) -> Result<usize, Errno> {
		let len = buf.len().min(self.sbi_buffer.len());
		if len == 0 {
			return Ok(0);
		}

		self.sbi_buffer[..len].copy_from_slice(&buf[..len]);
		let physical = Physical::<&[u8]>::new(len, self.sbi_buffer.as_ptr().addr(), 0);
		let written = sbi_rt::console_write(physical)
			.into_result()
			.map_err(|_| Errno::Io)?;

		if written > len {
			return Err(Errno::Io);
		}

		Ok(written)
	}
}

impl ErrorType for SerialDevice {
	type Error = Errno;
}

impl Read for SerialDevice {
	fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
		if buf.is_empty() {
			return Ok(0);
		}

		if let Some(byte) = self.buffered_byte.take() {
			buf[0] = byte;
			return Ok(1);
		}

		self.read_from_console(buf)
	}
}

impl ReadReady for SerialDevice {
	fn read_ready(&mut self) -> Result<bool, Self::Error> {
		if self.buffered_byte.is_none() {
			let mut byte = 0;
			if self.read_from_console(core::slice::from_mut(&mut byte))? == 1 {
				self.buffered_byte = Some(byte);
			}
		}

		Ok(self.buffered_byte.is_some())
	}
}

impl Write for SerialDevice {
	fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
		loop {
			let written = self.write_to_console(buf)?;
			if written > 0 || buf.is_empty() {
				return Ok(written);
			}
			hint::spin_loop();
		}
	}

	fn flush(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}
}
