use log::warn;

pub fn get_boot_time() -> u64 {
	warn!("`get_boot_time` is currently stubbed");
	0
}

/// Returns the current time in microseconds since UNIX epoch.
pub fn now_micros() -> u64 {
	get_boot_time() + super::processor::get_timer_ticks()
}
