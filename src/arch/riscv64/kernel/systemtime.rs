/// Returns the current time in microseconds since UNIX epoch.
pub fn now_micros() -> u64 {
	debug!("time is currently stubbed");
	super::processor::get_timer_ticks()
}
