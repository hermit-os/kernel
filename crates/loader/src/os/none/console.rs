use core::fmt;

use one_shot_mutex::OneShotMutex;

use crate::arch;

pub struct Console {
	console: Option<arch::Console>,
}

impl Console {
	const fn new() -> Self {
		Self { console: None }
	}

	#[cfg(target_arch = "aarch64")]
	pub fn get(&mut self) -> &mut arch::Console {
		self.console.get_or_insert_with(arch::Console::default)
	}
}

impl fmt::Write for Console {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		self.console
			.get_or_insert_with(arch::Console::default)
			.write_bytes(s.as_bytes());
		Ok(())
	}
}

pub static CONSOLE: OneShotMutex<Console> = OneShotMutex::new(Console::new());
