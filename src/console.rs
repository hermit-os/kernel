use core::task::Waker;
use core::{fmt, mem};

use hermit_sync::{InterruptTicketMutex, Lazy};

use crate::arch;

pub(crate) struct Console(pub arch::kernel::Console);

impl Console {
	fn new() -> Self {
		Self(arch::kernel::Console::new())
	}

	pub fn write(&mut self, buf: &[u8]) {
		self.0.write(buf);
	}

	pub fn read(&mut self) -> Option<u8> {
		self.0.read()
	}

	pub fn is_empty(&self) -> bool {
		self.0.is_empty()
	}

	pub fn register_waker(&mut self, waker: &Waker) {
		self.0.register_waker(waker);
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
