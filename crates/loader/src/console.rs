use core::fmt;

pub struct Console(());

impl fmt::Write for Console {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		for byte in s.bytes() {
			crate::arch::output_message_byte(byte);
		}
		Ok(())
	}
}

pub static mut CONSOLE: Console = Console(());
