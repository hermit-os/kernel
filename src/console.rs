use core::fmt;

use hermit_sync::InterruptTicketMutex;

use crate::arch;

pub(crate) struct Console(());

impl Console {
	pub fn write(&mut self, buf: &[u8]) {
		arch::output_message_buf(buf);
	}

	#[cfg(feature = "shell")]
	pub fn read(&mut self) -> Option<u8> {
		crate::arch::kernel::COM1
			.lock()
			.as_mut()
			.map(|s| s.read())?
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

pub(crate) static CONSOLE: InterruptTicketMutex<Console> = InterruptTicketMutex::new(Console(()));

#[doc(hidden)]
pub fn _print(args: fmt::Arguments<'_>) {
	use fmt::Write;
	CONSOLE.lock().write_fmt(args).unwrap();
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
	use super::*;

	#[test]
	fn test_console() {
		println!("HelloWorld");
	}
}
