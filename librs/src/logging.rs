// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//
// MIT License
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
// WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

#![allow(unused_macros)]

/// An enum representing the available verbosity levels of the logger.
#[derive(Copy, Clone)]
pub enum LogLevel {
	/// Disable all our put messages
	///
	/// Designates without information
	DISABLED = 0,
	/// The "error" level.
	///
	/// Designates very serious errors.
	ERROR,
	/// The "warn" level.
	///
	/// Designates hazardous situations.
 	WARNING,
	/// The "info" level.
	///
	/// Designates useful information.
 	INFO,
	// The "debug" level.
	///
	/// Designates lower priority information.
 	DEBUG
}

/// Data structures to filter kernel messages
pub struct KernelLogger {
	pub log_level: LogLevel,
}

/// default logger to handle kernel messages
pub static LOGGER: KernelLogger = KernelLogger { log_level: LogLevel::INFO };

/// Print formatted info text to our console, followed by a newline.
macro_rules! info {
	($fmt:expr) => ({
		let current_level = LOGGER.log_level as u8;
		let cmp_level = LogLevel::INFO as u8;

		if current_level >= cmp_level {
			println!(concat!("[INFO] ", $fmt));
		}
	});
	($fmt:expr, $($arg:tt)*) => ({
		let current_level = LOGGER.log_level as u8;
		let cmp_level = LogLevel::INFO as u8;

		if current_level >= cmp_level {
			println!(concat!("[INFO] ", $fmt), $($arg)*);
		}
	});
}

/// Print formatted warnings to our console, followed by a newline.
macro_rules! warn {
	($fmt:expr) => ({
		let current_level = LOGGER.log_level as u8;
		let cmp_level = LogLevel::WARNING as u8;

        if current_level >= cmp_level {
            println!(concat!("[WARNING] ", $fmt));
        }
    });
	($fmt:expr, $($arg:tt)*) => ({
		let current_level = LOGGER.log_level as u8;
		let cmp_level = LogLevel::WARNING  as u8;

		if current_level >= cmp_level {
			println!(concat!("[WARNING] ", $fmt), $($arg)*);
		}
	});
}

/// Print formatted warnings to our console, followed by a newline.
macro_rules! error {
	($fmt:expr) => ({
		let current_level = LOGGER.log_level as u8;
		let cmp_level = LogLevel::ERROR as u8;

        if current_level >= cmp_level {
            println!(concat!("[ERROR] ", $fmt));
        }
    });
	($fmt:expr, $($arg:tt)*) => ({
		let current_level = LOGGER.log_level as u8;
		let cmp_level = LogLevel::ERROR  as u8;

		if current_level >= cmp_level {
			println!(concat!("[ERROR] ", $fmt), $($arg)*);
		}
	});
}

/// Print formatted debuf messages to our console, followed by a newline.
macro_rules! debug {
	($fmt:expr) => ({
		let current_level = LOGGER.log_level as u8;
		let cmp_level = LogLevel::DEBUG as u8;

        if current_level >= cmp_level {
            println!(concat!("[DEBUG] ", $fmt));
        }
    });
	($fmt:expr, $($arg:tt)*) => ({
		let current_level = LOGGER.log_level as u8;
		let cmp_level = LogLevel::DEBUG  as u8;

		if current_level >= cmp_level {
			println!(concat!("[DEBUG] ", $fmt), $($arg)*);
		}
	});
}
