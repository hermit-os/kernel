// Copyright (c) 2017-2019 Stefan Lankes, RWTH Aachen University
//               2017 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use log::{set_logger_raw, LogLevelFilter, LogMetadata, LogRecord};

/// Data structure to filter kernel messages
struct KernelLogger;

impl log::Log for KernelLogger {
    fn enabled(&self, _: &LogMetadata) -> bool {
        true
    }

    fn log(&self, record: &LogRecord) {
        if self.enabled(record.metadata()) {
            println!("[{}][{}] {}", crate::arch::percore::core_id(), record.level(), record.args());
        }
    }
}

pub fn init() {
	unsafe {
        set_logger_raw(|max_log_level| {
            max_log_level.set(LogLevelFilter::Info);
            &KernelLogger
        }).expect("Can't initialize logger");
    }
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
