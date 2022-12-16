use core::fmt;

use hermit_sync::InterruptTicketMutex;

use crate::arch;

pub struct Console(());

/// A collection of methods that are required to format
/// a message to HermitCore's console.
impl fmt::Write for Console {
	/// Print a string of characters.
	#[inline]
	fn write_str(&mut self, s: &str) -> fmt::Result {
		if !s.is_empty() {
			let buf = s.as_bytes();
			arch::output_message_buf(buf);
		}

		Ok(())
	}
}

impl Console {
	#[inline]
	pub fn write_all(&mut self, buf: &[u8]) {
		arch::output_message_buf(buf)
	}
}

pub static CONSOLE: InterruptTicketMutex<Console> = InterruptTicketMutex::new(Console(()));

#[cfg(all(test, not(target_os = "none")))]
mod tests {
	use super::*;

	#[test]
	fn test_console() {
		println!("HelloWorld");
	}
}
