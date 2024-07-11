use core::fmt;

use alloc::collections::VecDeque;
use enum_dispatch::enum_dispatch;
use hermit_sync::InterruptTicketMutex;
use crate::io::{Read, Write};

use crate::kernel::serial::Serial;
use crate::{arch, io};

#[enum_dispatch]
pub trait SerialDevice: io::Read + io::Write {}

pub(crate) struct Console {
	serial: Option<Serial>,
	#[cfg(feature = "shell")]
	buffer: VecDeque<u8>,
}

impl Console {
	const fn empty() -> Self {
		Self {
			serial: None,
			#[cfg(feature = "shell")]
			buffer: VecDeque::new(),
		}
	}

	pub fn set_serial(&mut self, serial: Serial) {
		self.serial = Some(serial);
	}

	#[cfg(feature = "shell")]
	pub fn buffer_input(&mut self) {
		if let Some(serial) = self.serial.as_mut() {
			let mut buf = [0; 64];
			let n = serial.read(&mut buf).unwrap();
			self.buffer.extend(&buf[0..n]);
		}
	}
}

impl io::Read for Console {
	fn read(&mut self,buf: &mut [u8]) -> io::Result<usize> {
		self.buffer.read(buf)
	}
}

pub static CONSOLE: InterruptTicketMutex<Console> = InterruptTicketMutex::new(Console::empty());

#[doc(hidden)]
pub fn _print(args: ::core::fmt::Arguments<'_>) {
	let mut console = CONSOLE.lock();
	if let Some(serial) = &mut console.serial {
		serial.write_fmt(args).unwrap();
	}
}

pub fn buffer_input() {
	CONSOLE.lock().buffer_input();
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
	use super::*;

	#[test]
	fn test_console() {
		println!("HelloWorld");
	}
}
