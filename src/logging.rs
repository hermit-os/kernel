// Copyright (c) 2017-2019 Stefan Lankes, RWTH Aachen University
//               2017 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use log::{set_logger, set_max_level, LevelFilter, Metadata, Record};

/// Data structure to filter kernel messages
struct KernelLogger;

impl log::Log for KernelLogger {
	fn enabled(&self, _: &Metadata) -> bool {
		true
	}

	fn flush(&self) {
		// nothing to do
	}

	fn log(&self, record: &Record) {
		if self.enabled(record.metadata()) {
			println!(
				"[{}][{}] {}",
				crate::arch::percore::core_id(),
				record.level(),
				record.args()
			);
		}
	}
}

pub fn init() {
	set_logger(&KernelLogger).expect("Can't initialize logger");
	// Determines LevelFilter at compile time
	let log_level: Option<&'static str> = option_env!("HERMIT_LOG_LEVEL_FILTER");
	let max_level: LevelFilter = match log_level {
		Some("Error") => LevelFilter::Error,
		Some("Debug") => LevelFilter::Debug,
		Some("Off") => LevelFilter::Off,
		Some("Trace") => LevelFilter::Trace,
		Some("Warn") => LevelFilter::Warn,
		Some("Info") => LevelFilter::Info,
		_ => LevelFilter::Info,
	};
	set_max_level(max_level);
}

macro_rules! infoheader {
	// This should work on paper, but it's currently not supported :(
	// Refer to https://github.com/rust-lang/rust/issues/46569
	/*($($arg:tt)+) => ({
		info!("");
		info!("{:=^70}", format_args!($($arg)+));
	});*/
	($str:expr) => {{
		info!("");
		info!("{:=^70}", $str);
	}};
}

macro_rules! infoentry {
	($str:expr, $rhs:expr) => (infoentry!($str, "{}", $rhs));
	($str:expr, $($arg:tt)+) => (info!("{:25}{}", concat!($str, ":"), format_args!($($arg)+)));
}

macro_rules! infofooter {
	() => {{
		info!("{:=^70}", '=');
		info!("");
	}};
}
