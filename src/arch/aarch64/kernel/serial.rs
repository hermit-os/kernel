use core::arch::asm;

use embedded_io::{ErrorType, Write};

use crate::errno::Errno;
use crate::io;

pub(crate) struct SerialDevice {
	pub addr: u32,
}

impl SerialDevice {
	pub fn new() -> Self {
		let base = crate::env::boot_info()
			.hardware_info
			.serial_port_base
			.map(|uartport| uartport.get())
			.unwrap();

		Self { addr: base as u32 }
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
		let port = core::ptr::with_exposed_provenance_mut::<u8>(self.addr as usize);
		for &byte in buf {
			// LF newline characters need to be extended to CRLF over a real serial port.
			if byte == b'\n' {
				unsafe {
					asm!(
						"strb w8, [{port}]",
						port = in(reg) port,
						in("x8") b'\r',
						options(nostack),
					);
				}
			}

			unsafe {
				asm!(
					"strb w8, [{port}]",
					port = in(reg) port,
					in("x8") byte,
					options(nostack),
				);
			}
		}

		Ok(buf.len())
	}

	fn flush(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}
}
