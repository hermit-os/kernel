// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::arch::x86_64::kernel::irq;
use crate::arch::x86_64::kernel::processor;
use crate::arch::x86_64::kernel::BOOT_INFO;
use crate::environment;
use core::hint::spin_loop;
use x86::io::*;

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
		irq::disable();

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

	/**
	 * Returns the binary value for a given value in BCD (Binary-Coded Decimal).
	 */
	const fn convert_bcd_value(value: u8) -> u8 {
		((value / 16) * 10) + (value & 0xF)
	}

	/**
		* Returns the number of microseconds since the epoch from a given date.
		* Inspired by Linux Kernel's mktime64(), see kernel/time/time.c.
		*/
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

impl Drop for Rtc {
	fn drop(&mut self) {
		irq::enable();
	}
}

/**
 * Returns a (year, month, day, hour, minute, second) tuple from the given time in microseconds since the epoch.
 * Inspired from https://howardhinnant.github.io/date_algorithms.html#civil_from_days
 */
fn date_from_microseconds(microseconds_since_epoch: u64) -> (u16, u8, u8, u8, u8, u8) {
	let seconds_since_epoch = microseconds_since_epoch / 1_000_000;
	let second = (seconds_since_epoch % 60) as u8;
	let minutes_since_epoch = seconds_since_epoch / 60;
	let minute = (minutes_since_epoch % 60) as u8;
	let hours_since_epoch = minutes_since_epoch / 60;
	let hour = (hours_since_epoch % 24) as u8;
	let days_since_epoch = hours_since_epoch / 24;

	let days = days_since_epoch + 719_468;
	let era = days / 146_097;
	let day_of_era = days % 146_097;
	let year_of_era =
		(day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146_096) / 365;
	let mut year = (year_of_era + era * 400) as u16;
	let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
	let internal_month = (5 * day_of_year + 2) / 153;
	let day = (day_of_year - (153 * internal_month + 2) / 5 + 1) as u8;

	let mut month = internal_month as u8;
	if internal_month < 10 {
		month += 3;
	} else {
		month -= 9;
	}

	if month <= 2 {
		year += 1;
	}

	(year, month, day, hour, minute, second)
}

pub fn get_boot_time() -> u64 {
	unsafe { core::ptr::read_volatile(&(*BOOT_INFO).boot_gtod) }
}

pub fn init() {
	let mut microseconds_offset = get_boot_time();

	if microseconds_offset == 0 && !environment::is_uhyve() {
		// Get the current time in microseconds since the epoch (1970-01-01) from the x86 RTC.
		// Subtract the timer ticks to get the actual time when HermitCore-rs was booted.
		let rtc = Rtc::new();
		microseconds_offset = rtc.get_microseconds_since_epoch() - processor::get_timer_ticks();
		unsafe { core::ptr::write_volatile(&mut (*BOOT_INFO).boot_gtod, microseconds_offset) }
	}

	let (year, month, day, hour, minute, second) = date_from_microseconds(microseconds_offset);
	info!(
		"HermitCore-rs booted on {:04}-{:02}-{:02} at {:02}:{:02}:{:02}",
		year, month, day, hour, minute, second
	);
}
