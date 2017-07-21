// Copyright (c) 2017, Stefan Lankes, RWTH Aachen University
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are met:
//    * Redistributions of source code must retain the above copyright
//      notice, this list of conditions and the following disclaimer.
//    * Redistributions in binary form must reproduce the above copyright
//      notice, this list of conditions and the following disclaimer in the
//      documentation and/or other materials provided with the distribution.
//    * Neither the name of the University nor the names of its contributors
//      may be used to endorse or promote products derived from this
//      software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
// ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
// WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE FOR ANY
// DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
// (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
// LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND
// ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
// (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
// SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

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
