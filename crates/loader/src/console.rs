use crate::arch;
use core::fmt;

pub struct Console;

/// A collection of methods that are required to format
/// a message to HermitCore's console.
impl fmt::Write for Console {
	/// Print a single character.
	fn write_char(&mut self, c: char) -> fmt::Result {
		arch::output_message_byte(c as u8);
		Ok(())
	}

	/// Print a string of characters.
	fn write_str(&mut self, s: &str) -> fmt::Result {
		for character in s.chars() {
			self.write_char(character).unwrap();
		}
		Ok(())
	}
}
