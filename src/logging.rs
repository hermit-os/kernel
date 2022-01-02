use log::{set_logger_racy, set_max_level, LevelFilter, Metadata, Record};

/// Data structure to filter kernel messages
struct KernelLogger;

impl log::Log for KernelLogger {
	fn enabled(&self, _: &Metadata<'_>) -> bool {
		true
	}

	fn flush(&self) {
		// nothing to do
	}

	fn log(&self, record: &Record<'_>) {
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

pub unsafe fn init() {
	set_logger_racy(&KernelLogger).expect("Can't initialize logger");
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

#[macro_export]
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

#[macro_export]
macro_rules! infoentry {
	($str:expr, $rhs:expr) => (crate::infoentry!($str, "{}", $rhs));
	($str:expr, $($arg:tt)+) => (::log::info!("{:25}{}", concat!($str, ":"), format_args!($($arg)+)));
}

#[macro_export]
macro_rules! infofooter {
	() => {{
		::log::info!("{:=^70}", '=');
		::log::info!("");
	}};
}
