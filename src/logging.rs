use core::fmt;

use anstyle::AnsiColor;
use log::{Level, LevelFilter, Metadata, Record};

pub static KERNEL_LOGGER: KernelLogger = KernelLogger;

/// Data structure to filter kernel messages
pub struct KernelLogger;

impl log::Log for KernelLogger {
	fn enabled(&self, _: &Metadata<'_>) -> bool {
		true
	}

	fn flush(&self) {
		// nothing to do
	}

	fn log(&self, record: &Record<'_>) {
		if !self.enabled(record.metadata()) {
			return;
		}

		let core_id = crate::arch::core_local::core_id();
		let level = ColorLevel(record.level());
		let args = record.args();
		println!("[{core_id}][{level}] {args}");
	}
}

struct ColorLevel(Level);

impl fmt::Display for ColorLevel {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let level = self.0;

		if no_color() {
			write!(f, "{level}")
		} else {
			let color = match level {
				Level::Trace => AnsiColor::Magenta,
				Level::Debug => AnsiColor::Blue,
				Level::Info => AnsiColor::Green,
				Level::Warn => AnsiColor::Yellow,
				Level::Error => AnsiColor::Red,
			};

			let style = anstyle::Style::new().fg_color(Some(color.into()));
			write!(f, "{style}{level}{style:#}")
		}
	}
}

fn no_color() -> bool {
	option_env!("NO_COLOR").is_some_and(|val| !val.is_empty())
}

pub unsafe fn init() {
	log::set_logger(&KERNEL_LOGGER).expect("Can't initialize logger");
	// Determines LevelFilter at compile time
	let log_level: Option<&'static str> = option_env!("HERMIT_LOG_LEVEL_FILTER");
	let mut max_level = LevelFilter::Info;

	if let Some(log_level) = log_level {
		max_level = if log_level.eq_ignore_ascii_case("off") {
			LevelFilter::Off
		} else if log_level.eq_ignore_ascii_case("error") {
			LevelFilter::Error
		} else if log_level.eq_ignore_ascii_case("warn") {
			LevelFilter::Warn
		} else if log_level.eq_ignore_ascii_case("info") {
			LevelFilter::Info
		} else if log_level.eq_ignore_ascii_case("debug") {
			LevelFilter::Debug
		} else if log_level.eq_ignore_ascii_case("trace") {
			LevelFilter::Trace
		} else {
			error!("Could not parse HERMIT_LOG_LEVEL_FILTER, falling back to `info`.");
			LevelFilter::Info
		};
	}

	log::set_max_level(max_level);
}

#[cfg_attr(target_arch = "riscv64", allow(unused))]
macro_rules! infoheader {
	// This should work on paper, but it's currently not supported :(
	// Refer to https://github.com/rust-lang/rust/issues/46569
	/*($($arg:tt)+) => ({
		info!("");
		info!("{:=^70}", format_args!($($arg)+));
	});*/
	($str:expr) => {{
		::log::info!("");
		::log::info!("{:=^70}", $str);
	}};
}

#[cfg_attr(target_arch = "riscv64", allow(unused))]
macro_rules! infoentry {
	($str:expr, $rhs:expr) => (infoentry!($str, "{}", $rhs));
	($str:expr, $($arg:tt)+) => (::log::info!("{:25}{}", concat!($str, ":"), format_args!($($arg)+)));
}

#[cfg_attr(target_arch = "riscv64", allow(unused))]
macro_rules! infofooter {
	() => {{
		::log::info!("{:=^70}", '=');
		::log::info!("");
	}};
}
