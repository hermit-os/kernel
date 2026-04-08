use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};
use core::time::Duration;

use anstyle::AnsiColor;
use hermit_sync::OnceCell;
use log::{Level, LevelFilter, Metadata, Record};

pub static KERNEL_LOGGER: KernelLogger = KernelLogger::new();
const ARENA_SIZE: usize = 4096;
static mut ARENA: [u8; ARENA_SIZE] = [0; _];

const TIME_SEC_WIDTH: usize = 5;
const TIME_SUBSEC_WIDTH: usize = 6;

/// Data structure to filter kernel messages
pub struct KernelLogger {
	time: AtomicBool,
	filter: OnceCell<env_filter::Filter>,
}

impl KernelLogger {
	pub const fn new() -> Self {
		Self {
			time: AtomicBool::new(false),
			filter: OnceCell::new(),
		}
	}

	pub fn time(&self) -> bool {
		self.time.load(Ordering::Relaxed)
	}

	pub fn set_time(&self, time: bool) {
		self.time.store(time, Ordering::Relaxed);
	}
}

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

		if let Some(filter) = self.filter.get()
			&& !filter.matches(record)
		{
			return;
		}

		let format_time = if self.time() {
			let time = Duration::from_micros(crate::processor::get_timer_ticks());
			format_args!(
				"{:TIME_SEC_WIDTH$}.{:0TIME_SUBSEC_WIDTH$}",
				time.as_secs(),
				time.subsec_micros()
			)
		} else {
			format_args!("{:1$}", "", TIME_SEC_WIDTH + 1 + TIME_SUBSEC_WIDTH)
		};
		let core_id = crate::arch::core_local::core_id();
		let level = ColorLevel(record.level());

		let target = record.target();
		let (crate_, modules) = target.split_once("::").unwrap_or((target, ""));
		let (_modules, module) = modules.rsplit_once("::").unwrap_or(("", modules));
		let target = if !module.is_empty() && crate_ == "hermit" {
			module
		} else {
			crate_
		};
		let format_target = format_args!(" {target:<10}");

		let args = record.args();
		println!("[{format_time}][{core_id}][{level}{format_target}] {args}");
	}
}

struct ColorLevel(Level);

impl fmt::Display for ColorLevel {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let level = self.0;

		if no_color() {
			write!(f, "{level:<5}")
		} else {
			let color = match level {
				Level::Trace => AnsiColor::Magenta,
				Level::Debug => AnsiColor::Blue,
				Level::Info => AnsiColor::Green,
				Level::Warn => AnsiColor::Yellow,
				Level::Error => AnsiColor::Red,
			};

			let style = anstyle::Style::new().fg_color(Some(color.into()));
			write!(f, "{style}{level:<5}{style:#}")
		}
	}
}

fn no_color() -> bool {
	option_env!("NO_COLOR").is_some_and(|val| !val.is_empty())
}

pub unsafe fn init() {
	#[cfg(target_os = "none")]
	unsafe {
		crate::mm::ALLOCATOR
			.lock()
			.claim((&raw mut ARENA).cast(), ARENA_SIZE)
			.unwrap()
	};

	log::set_logger(&KERNEL_LOGGER).expect("Can't initialize logger");
	// Determines LevelFilter at compile time
	let log_level: Option<&'static str> = option_env!("HERMIT_LOG_LEVEL_FILTER");
	let max_level = if let Some(log_level) = log_level {
		let filter = env_filter::Builder::new()
			// The default. It may get overwritten by the parsed filter if it has a global level.
			.filter_level(LevelFilter::Info)
			.parse(log_level)
			.build();
		let max_level = filter.filter();
		KERNEL_LOGGER.filter.set(filter).unwrap();
		max_level
	} else {
		LevelFilter::Info
	};

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
#[clippy::format_args]
macro_rules! infoentry {
	($str:expr, $($arg:tt)+) => (::log::info!("{:25}{}", concat!($str, ":"), format_args!($($arg)+)));
}

#[cfg_attr(target_arch = "riscv64", allow(unused))]
macro_rules! infofooter {
	() => {{
		::log::info!("{:=^70}", '=');
		::log::info!("");
	}};
}
