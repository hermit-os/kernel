// Copyright (c) 2017-2019 Stefan Lankes, RWTH Aachen University
//               2017 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use log::{Record, Level, Metadata, LevelFilter};

/// Data structures to filter kernel messages
struct KernelLogger;

/// default logger to handle kernel messages
static LOGGER: KernelLogger = KernelLogger;

impl log::Log for KernelLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            println!("[{}][{}] {}", crate::arch::percore::core_id(), record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

pub fn init() {
    log::set_logger(&LOGGER)
        .map(|()| log::set_max_level(LevelFilter::Info)).unwrap();
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
