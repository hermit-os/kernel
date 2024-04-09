use core::fmt;

use one_shot_mutex::OneShotMutex;

pub struct Console(());

impl fmt::Write for Console {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		crate::arch::write_to_console(s.as_bytes());
		Ok(())
	}
}

pub static CONSOLE: OneShotMutex<Console> = OneShotMutex::new(Console(()));
