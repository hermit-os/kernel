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
pub static LOGGER: KernelLogger = KernelLogger { log_level: LogLevel::Debug };


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
