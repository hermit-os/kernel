// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//               2017 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

/// An enum representing the available verbosity levels of the logger.
#[allow(dead_code)]
#[derive(Copy, Clone)]
pub enum LogLevel {
	/// Disable all our put messages
	///
	/// Designates without information
	Disabled = 0,
	/// The "error" level.
	///
	/// Designates very serious errors.
	Error,
	/// The "warn" level.
	///
	/// Designates hazardous situations.
	Warning,
	/// The "info" level.
	///
	/// Designates useful information.
	Info,
	// The "debug" level.
	///
	/// Designates lower priority information.
	Debug,
	// The "debug_mem" level.
	///
	/// Designates lower priority information of the memory management (high frequency!).
	DebugMem,
}

/// Data structures to filter kernel messages
pub struct KernelLogger {
	pub log_level: LogLevel,
}

/// default logger to handle kernel messages
pub const LOGGER: KernelLogger = KernelLogger { log_level: LogLevel::Info };


macro_rules! printlog {
	($type:expr, $cmp_level:expr, $($arg:tt)+) => ({
		let current_level = $crate::logging::LOGGER.log_level as u8;

		if current_level >= ($cmp_level as u8) {
			println!("[{}][{}] {}", $crate::arch::percore::core_id(), $type, format_args!($($arg)+));
		}
	});
}

/// Print formatted info text to our console, followed by a newline.
macro_rules! info {
	($($arg:tt)+) => (printlog!("INFO", $crate::logging::LogLevel::Info, $($arg)+));
}

macro_rules! infoheader {
	// This should work on paper, but it's currently not supported :(
	// Refer to https://github.com/rust-lang/rust/issues/46569
	/*($($arg:tt)+) => ({
		info!("");
		info!("{:=^70}", format_args!($($arg)+));
	});*/
	($str:expr) => ({
		info!("");
		info!("{:=^70}", $str);
	});
}

macro_rules! infoentry {
	($str:expr, $rhs:expr) => (infoentry!($str, "{}", $rhs));
	($str:expr, $($arg:tt)+) => (info!("{:25}{}", concat!($str, ":"), format_args!($($arg)+)));
}

macro_rules! infofooter {
	() => ({
		info!("{:=^70}", '=');
		info!("");
	});
}

/// Print formatted warnings to our console, followed by a newline.
macro_rules! warn {
	($($arg:tt)+) => (printlog!("WARNING", $crate::logging::LogLevel::Warning, $($arg)+));
}

/// Print formatted warnings to our console, followed by a newline.
macro_rules! error {
	($($arg:tt)+) => (printlog!("ERROR", $crate::logging::LogLevel::Error, $($arg)+));
}

/// Print formatted debug messages to our console, followed by a newline.
macro_rules! debug {
	($($arg:tt)+) => (printlog!("DEBUG", $crate::logging::LogLevel::Debug, $($arg)+));
}

/// Print formatted debug messages to our console, followed by a newline.
macro_rules! debug_mem {
	($($arg:tt)+) => (printlog!("DEBUG_MEM", $crate::logging::LogLevel::DebugMem, $($arg)+));
}
