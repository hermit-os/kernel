use crate::arch;

/// Represent the number of seconds and microseconds since
/// the Epoch (1970-01-01 00:00:00 +0000 (UTC))
#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct timeval {
	/// seconds
	pub tv_sec: i64,
	/// microseconds
	pub tv_usec: i64,
}

impl timeval {
	pub fn from_usec(microseconds: u64) -> Self {
		Self {
			tv_sec: (microseconds / 1_000_000) as i64,
			tv_usec: (microseconds % 1_000_000) as i64,
		}
	}

	pub fn into_usec(&self) -> Option<u64> {
		u64::try_from(self.tv_sec)
			.ok()
			.and_then(|secs| secs.checked_mul(1_000_000))
			.and_then(|millions| millions.checked_add(u64::try_from(self.tv_usec).ok()?))
	}
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct itimerval {
	pub it_interval: timeval,
	pub it_value: timeval,
}

/// Represent the number of seconds and nanoseconds since
/// the Epoch (1970-01-01 00:00:00 +0000 (UTC))
#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct timespec {
	/// seconds
	pub tv_sec: i64,
	/// nanoseconds
	pub tv_nsec: i64,
}

impl timespec {
	pub fn from_usec(microseconds: u64) -> Self {
		Self {
			tv_sec: (microseconds / 1_000_000) as i64,
			tv_nsec: ((microseconds % 1_000_000) * 1000) as i64,
		}
	}

	pub fn into_usec(&self) -> Option<u64> {
		u64::try_from(self.tv_sec)
			.ok()
			.and_then(|secs| secs.checked_mul(1_000_000))
			.and_then(|millions| millions.checked_add(u64::try_from(self.tv_nsec).ok()? / 1000))
	}
}

#[derive(Copy, Clone, Debug)]
pub struct SystemTime(timespec);

impl SystemTime {
	/// Returns the system time corresponding to "now".
	pub fn now() -> Self {
		let microseconds = arch::processor::get_timer_ticks() + arch::get_boot_time();

		Self(timespec::from_usec(microseconds))
	}
}

impl From<timespec> for SystemTime {
	fn from(t: timespec) -> Self {
		Self(t)
	}
}
