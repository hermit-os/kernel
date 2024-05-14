use crate::arch;

#[allow(non_camel_case_types)]
pub type time_t = i64;
#[allow(non_camel_case_types)]
pub type useconds_t = u32;
#[allow(non_camel_case_types)]
pub type suseconds_t = i32;

/// Represent the number of seconds and microseconds since
/// the Epoch (1970-01-01 00:00:00 +0000 (UTC))
#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct timeval {
	/// seconds
	pub tv_sec: time_t,
	/// microseconds
	pub tv_usec: suseconds_t,
}

impl timeval {
	pub fn from_usec(microseconds: i64) -> Self {
		Self {
			tv_sec: (microseconds / 1_000_000),
			tv_usec: (microseconds % 1_000_000) as i32,
		}
	}

	pub fn into_usec(&self) -> Option<i64> {
		self.tv_sec
			.checked_mul(1_000_000)
			.and_then(|usec| usec.checked_add(self.tv_usec.into()))
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
#[derive(Copy, Clone, Debug, Default)]
#[repr(C)]
pub struct timespec {
	/// seconds
	pub tv_sec: time_t,
	/// nanoseconds
	pub tv_nsec: i32,
}

impl timespec {
	pub fn from_usec(microseconds: i64) -> Self {
		Self {
			tv_sec: (microseconds / 1_000_000),
			tv_nsec: ((microseconds % 1_000_000) * 1000) as i32,
		}
	}

	pub fn into_usec(&self) -> Option<i64> {
		self.tv_sec
			.checked_mul(1_000_000)
			.and_then(|usec| usec.checked_add((self.tv_nsec / 1000).into()))
	}
}

#[derive(Copy, Clone, Debug, Default)]
pub struct SystemTime(timespec);

impl SystemTime {
	/// Returns the system time corresponding to "now".
	pub fn now() -> Self {
		Self(timespec::from_usec(
			arch::kernel::systemtime::now_micros() as i64
		))
	}
}

impl From<timespec> for SystemTime {
	fn from(t: timespec) -> Self {
		Self(t)
	}
}

impl From<SystemTime> for timespec {
	fn from(value: SystemTime) -> Self {
		value.0
	}
}
