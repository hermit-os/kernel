use core::fmt;

use anstyle::AnsiColor;
use log::{Level, LevelFilter, Metadata, Record};

struct Logger;

impl log::Log for Logger {
	fn enabled(&self, metadata: &Metadata<'_>) -> bool {
		let level = option_env!("LOADER_LOG")
			.map(|var| var.parse().unwrap())
			.unwrap_or(Level::Info);
		metadata.level() <= level
	}

	fn log(&self, record: &Record<'_>) {
		if self.enabled(record.metadata()) {
			let level = ColorLevel(record.level());
			let args = record.args();
			println!("[LOADER][{level}] {args}");
		}
	}

	fn flush(&self) {}
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

pub fn init() {
	static LOGGER: Logger = Logger;
	log::set_logger(&LOGGER).unwrap();
	let level_filter = option_env!("LOADER_LOG")
		.map(|var| var.parse().unwrap())
		.unwrap_or(LevelFilter::Info);
	log::set_max_level(level_filter);
}
