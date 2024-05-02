use core::fmt;

use hermit_sync::InterruptTicketMutex;

use crate::arch;

pub(crate) struct Console(());

/// A collection of methods that are required to format
/// a message to Hermit's console.
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

static CONSOLE: InterruptTicketMutex<Console> = InterruptTicketMutex::new(Console(()));

#[doc(hidden)]
pub fn _print(args: ::core::fmt::Arguments<'_>) {
	use core::fmt::Write;
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
