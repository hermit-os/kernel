use core::fmt;

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

cfg_select! {
	target_arch = "aarch64" => {
		use super::unsound_mutex::UnsoundMutex;

		// FIXME: remove this once we have early page tables on ARM.
		pub static CONSOLE: UnsoundMutex<Console> = UnsoundMutex::new(Console::new());
	}
	_ => {
		use one_shot_mutex::sync::OneShotMutex;

		pub static CONSOLE: OneShotMutex<Console> = OneShotMutex::new(Console::new());
	}
}
