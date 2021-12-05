use crate::arch;
use crate::synch::spinlock::SpinlockIrqSave;
use core::fmt;

pub struct Console(());

/// A collection of methods that are required to format
/// a message to HermitCore's console.
impl fmt::Write for Console {
	/// Print a string of characters.
	fn write_str(&mut self, s: &str) -> fmt::Result {
		if !s.is_empty() {
			let buf = s.as_bytes();
			arch::output_message_buf(buf);
		}

		Ok(())
	}

	/// Print a single character.
	fn write_char(&mut self, c: char) -> fmt::Result {
		self.write_str(c.encode_utf8(&mut [0; 4]))
	}
}

impl Console {
	pub fn write_all(&mut self, buf: &[u8]) {
		arch::output_message_buf(buf)
	}
}

pub static CONSOLE: SpinlockIrqSave<Console> = SpinlockIrqSave::new(Console(()));

#[cfg(not(any(target_os = "none", target_os = "hermit")))]
#[test]
fn test_console() {
	println!("HelloWorld");
}
