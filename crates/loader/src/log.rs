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
			let level = record.level();
			let args = record.args();
			println!("[LOADER][{level}] {args}");
		}
	}

	fn flush(&self) {}
}

pub fn init() {
	static LOGGER: Logger = Logger;
	log::set_logger(&LOGGER).unwrap();
	let level_filter = option_env!("LOADER_LOG")
		.map(|var| var.parse().unwrap())
		.unwrap_or(LevelFilter::Info);
	log::set_max_level(level_filter);
}
