// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//               2017 Colin Finck, RWTH Aachen University
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
pub static LOGGER: KernelLogger = KernelLogger { log_level: LogLevel::DEBUG };


macro_rules! printlog {
	($fmt:expr, $type:expr, $cmp_level:expr) => ({
		let current_level = $crate::logging::LOGGER.log_level as u8;

		if current_level >= ($cmp_level as u8) {
			println!(concat!("[{}][{}] ", $fmt), $crate::arch::percore::core_id(), $type);
		}
	});
	($fmt:expr, $type:expr, $cmp_level:expr, $($arg:tt)*) => ({
		let current_level = $crate::logging::LOGGER.log_level as u8;

		if current_level >= ($cmp_level as u8) {
			println!(concat!("[{}][{}] ", $fmt), $crate::arch::percore::core_id(), $type, $($arg)*);
		}
	});
}

/// Print formatted info text to our console, followed by a newline.
macro_rules! info {
	($fmt:expr) => (printlog!($fmt, "INFO", $crate::logging::LogLevel::INFO));
	($fmt:expr, $($arg:tt)*) => (printlog!($fmt, "INFO", $crate::logging::LogLevel::INFO, $($arg)*));
}

/// Print formatted warnings to our console, followed by a newline.
macro_rules! warn {
	($fmt:expr) => (printlog!($fmt, "WARNING", $crate::logging::LogLevel::WARNING));
	($fmt:expr, $($arg:tt)*) => (printlog!($fmt, "WARNING", $crate::logging::LogLevel::WARNING, $($arg)*));
}

/// Print formatted warnings to our console, followed by a newline.
macro_rules! error {
	($fmt:expr) => (printlog!($fmt, "ERROR", $crate::logging::LogLevel::ERROR));
	($fmt:expr, $($arg:tt)*) => (printlog!($fmt, "ERROR", $crate::logging::LogLevel::ERROR, $($arg)*));
}

/// Print formatted debuf messages to our console, followed by a newline.
macro_rules! debug {
	($fmt:expr) => (printlog!($fmt, "DEBUG", $crate::logging::LogLevel::DEBUG));
	($fmt:expr, $($arg:tt)*) => (printlog!($fmt, "DEBUG", $crate::logging::LogLevel::DEBUG, $($arg)*));
}
