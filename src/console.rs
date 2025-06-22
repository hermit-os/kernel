use core::task::Waker;
use core::{fmt, mem};

use heapless::Vec;
use hermit_sync::{InterruptTicketMutex, Lazy};

use crate::arch;

const SERIAL_BUFFER_SIZE: usize = 256;

pub(crate) struct Console {
	pub inner: arch::kernel::Console,
	buffer: Vec<u8, SERIAL_BUFFER_SIZE>,
}

impl Console {
	fn new() -> Self {
		Self {
			inner: arch::kernel::Console::new(),
			buffer: Vec::new(),
		}
	}

	/// Writes a buffer to the console.
	/// The content is buffered until a newline is encountered or the internal buffer is full.
	/// To force early output, use [`flush`](Self::flush).
	pub fn write(&mut self, buf: &[u8]) {
		if SERIAL_BUFFER_SIZE - self.buffer.len() >= buf.len() {
			// unwrap: we checked that buf fits in self.buffer
			self.buffer.extend_from_slice(buf).unwrap();
			if buf.contains(&b'\n') {
				self.flush();
			}
		} else {
			self.inner.write(&self.buffer);
			self.buffer.clear();
			if buf.len() >= SERIAL_BUFFER_SIZE {
				self.inner.write(buf);
			} else {
				// unwrap: we checked that buf fits in self.buffer
				self.buffer.extend_from_slice(buf).unwrap();
				if buf.contains(&b'\n') {
					self.flush();
				}
			}
		}
	}

	/// Immediately writes everything in the internal buffer to the output.
	pub fn flush(&mut self) {
		self.inner.write(&self.buffer);
		self.buffer.clear();
	}

	pub fn read(&mut self) -> Option<u8> {
		self.inner.read()
	}

	pub fn is_empty(&self) -> bool {
		self.inner.is_empty()
	}

	pub fn register_waker(&mut self, waker: &Waker) {
		self.inner.register_waker(waker);
	}
}

/// A collection of methods that are required to format
/// a message to Hermit's console.
impl fmt::Write for Console {
	/// Print a string of characters.
	#[inline]
	fn write_str(&mut self, s: &str) -> fmt::Result {
		if !s.is_empty() {
			self.write(s.as_bytes());
		}

		Ok(())
	}
}

pub(crate) static CONSOLE: Lazy<InterruptTicketMutex<Console>> =
	Lazy::new(|| InterruptTicketMutex::new(Console::new()));

#[doc(hidden)]
pub fn _print(args: fmt::Arguments<'_>) {
	use fmt::Write;
	CONSOLE.lock().write_fmt(args).unwrap();
}

#[doc(hidden)]
pub fn _panic_print(args: fmt::Arguments<'_>) {
	use fmt::Write;
	let mut console = unsafe { CONSOLE.make_guard_unchecked() };
	console.write_fmt(args).ok();
	mem::forget(console);
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
	use super::*;

	#[test]
	fn test_console() {
		println!("HelloWorld");
	}
}
