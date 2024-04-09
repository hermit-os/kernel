use core::fmt;

use one_shot_mutex::OneShotMutex;

pub struct Console(());

impl fmt::Write for Console {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		#[cfg(target_os = "none")]
		crate::arch::write_to_console(s.as_bytes());
		#[cfg(target_os = "uefi")]
		uefi_services::system_table().stdout().write_str(s)?;
		Ok(())
	}
}

pub static CONSOLE: OneShotMutex<Console> = OneShotMutex::new(Console(()));
