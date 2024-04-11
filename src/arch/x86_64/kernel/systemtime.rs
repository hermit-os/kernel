use core::hint::spin_loop;

use hermit_entry::boot_info::PlatformInfo;
use hermit_sync::{without_interrupts, OnceCell};
use time::OffsetDateTime;
use x86::io::*;

use crate::arch::x86_64::kernel::{boot_info, processor};

const CMOS_COMMAND_PORT: u16 = 0x70;
const CMOS_DATA_PORT: u16 = 0x71;

const CMOS_DISABLE_NMI: u8 = 1 << 7;

const CMOS_SECOND_REGISTER: u8 = 0x00;
const CMOS_MINUTE_REGISTER: u8 = 0x02;
const CMOS_HOUR_REGISTER: u8 = 0x04;
const CMOS_DAY_REGISTER: u8 = 0x07;
const CMOS_MONTH_REGISTER: u8 = 0x08;
const CMOS_YEAR_REGISTER: u8 = 0x09;
const CMOS_STATUS_REGISTER_A: u8 = 0x0A;
const CMOS_STATUS_REGISTER_B: u8 = 0x0B;

const CMOS_UPDATE_IN_PROGRESS_FLAG: u8 = 1 << 7;
const CMOS_24_HOUR_FORMAT_FLAG: u8 = 1 << 1;
const CMOS_BINARY_FORMAT_FLAG: u8 = 1 << 2;
const CMOS_12_HOUR_PM_FLAG: u8 = 0x80;

struct Rtc {
	cmos_format: u8,
}

impl Rtc {
	fn new() -> Self {
		Self {
			cmos_format: Self::read_cmos_register(CMOS_STATUS_REGISTER_B),
		}
	}

	const fn is_24_hour_format(&self) -> bool {
		self.cmos_format & CMOS_24_HOUR_FORMAT_FLAG > 0
	}

	const fn is_binary_format(&self) -> bool {
		self.cmos_format & CMOS_BINARY_FORMAT_FLAG > 0
	}

	const fn time_is_pm(hour: u8) -> bool {
		hour & CMOS_12_HOUR_PM_FLAG > 0
	}

	/// Returns the binary value for a given value in BCD (Binary-Coded Decimal).
	const fn convert_bcd_value(value: u8) -> u8 {
		((value / 16) * 10) + (value & 0xF)
	}

	/// Returns the number of microseconds since the epoch from a given date.
	/// Inspired by Linux Kernel's mktime64(), see kernel/time/time.c.
	fn microseconds_from_date(
		year: u16,
		month: u8,
		day: u8,
		hour: u8,
		minute: u8,
		second: u8,
	) -> u64 {
		let (m, y) = if month > 2 {
			(u64::from(month - 2), u64::from(year))
		} else {
			(u64::from(month + 12 - 2), u64::from(year - 1))
		};

		let days_since_epoch =
			(y / 4 - y / 100 + y / 400 + 367 * m / 12 + u64::from(day)) + y * 365 - 719_499;
		let hours_since_epoch = days_since_epoch * 24 + u64::from(hour);
		let minutes_since_epoch = hours_since_epoch * 60 + u64::from(minute);
		let seconds_since_epoch = minutes_since_epoch * 60 + u64::from(second);

		seconds_since_epoch * 1_000_000u64
	}

	fn read_cmos_register(register: u8) -> u8 {
		unsafe {
			outb(CMOS_COMMAND_PORT, CMOS_DISABLE_NMI | register);
			inb(CMOS_DATA_PORT)
		}
	}

	fn read_datetime_register(&self, register: u8) -> u8 {
		let value = Self::read_cmos_register(register);

		// Every date/time register may either be in binary or in BCD format.
		// Convert BCD values if necessary.
		if self.is_binary_format() {
			value
		} else {
			Self::convert_bcd_value(value)
		}
	}

	fn read_all_values(&self) -> u64 {
		// Reading year, month, and day is straightforward.
		let year = u16::from(self.read_datetime_register(CMOS_YEAR_REGISTER)) + 2000;
		let month = self.read_datetime_register(CMOS_MONTH_REGISTER);
		let day = self.read_datetime_register(CMOS_DAY_REGISTER);

		// The hour register is a bitch.
		// On top of being in either binary or BCD format, it may also be in 12-hour
		// or 24-hour format.
		let mut hour = Self::read_cmos_register(CMOS_HOUR_REGISTER);
		let mut is_pm = false;

		// Check and mask off a potential PM flag if the hour is given in 12-hour format.
		if !self.is_24_hour_format() {
			is_pm = Self::time_is_pm(hour);
			hour &= !CMOS_12_HOUR_PM_FLAG;
		}

		// Now convert a BCD number to binary if necessary (after potentially masking off the PM flag above).
		if !self.is_binary_format() {
			hour = Self::convert_bcd_value(hour);
		}

		// If the hour is given in 12-hour format, do the necessary calculations to convert it into 24 hours.
		if !self.is_24_hour_format() {
			if hour == 12 {
				// 12:00 AM is 00:00 and 12:00 PM is 12:00 (see is_pm below) in 24-hour format.
				hour = 0;
			}

			if is_pm {
				// {01:00 PM, 02:00 PM, ...} is {13:00, 14:00, ...} in 24-hour format.
				hour += 12;
			}
		}

		// The minute and second registers are straightforward again.
		let minute = self.read_datetime_register(CMOS_MINUTE_REGISTER);
		let second = self.read_datetime_register(CMOS_SECOND_REGISTER);

		// Convert it all to microseconds and return the result.
		Self::microseconds_from_date(year, month, day, hour, minute, second)
	}

	pub fn get_microseconds_since_epoch(&self) -> u64 {
		loop {
			// If a clock update is currently in progress, wait until it is finished.
			while Self::read_cmos_register(CMOS_STATUS_REGISTER_A) & CMOS_UPDATE_IN_PROGRESS_FLAG
				> 0
			{
				spin_loop();
			}

			// Get the current time in microseconds since the epoch.
			let microseconds_since_epoch_1 = self.read_all_values();

			// If the clock is already updating the time again, the read values may be inconsistent
			// and we have to repeat this process.
			if Self::read_cmos_register(CMOS_STATUS_REGISTER_A) & CMOS_UPDATE_IN_PROGRESS_FLAG > 0 {
				continue;
			}

			// Get the current time again and verify that it's the same we last read.
			let microseconds_since_epoch_2 = self.read_all_values();
			if microseconds_since_epoch_1 == microseconds_since_epoch_2 {
				// Both times are identical, so we have read consistent values and can exit the loop.
				return microseconds_since_epoch_1;
			}
		}
	}
}

static BOOT_TIME: OnceCell<u64> = OnceCell::new();

pub fn init() {
	let boot_time = match boot_info().platform_info {
		PlatformInfo::Uhyve { boot_time, .. } => boot_time,
		_ => {
			// Get the current time in microseconds since the epoch (1970-01-01) from the x86 RTC.
			// Subtract the timer ticks to get the actual time when Hermit was booted.
			let current_time = without_interrupts(|| Rtc::new().get_microseconds_since_epoch());
			let boot_time = current_time - processor::get_timer_ticks();
			OffsetDateTime::from_unix_timestamp_nanos(boot_time as i128 * 1000).unwrap()
		}
	};
	info!("Hermit booted on {boot_time}");

	let micros = u64::try_from(boot_time.unix_timestamp_nanos() / 1000).unwrap();
	BOOT_TIME.set(micros).unwrap();
}

/// Returns the current time in microseconds since UNIX epoch.
pub fn now_micros() -> u64 {
	*BOOT_TIME.get().unwrap() + super::processor::get_timer_ticks()
}
