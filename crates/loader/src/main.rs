#![no_std]
#![no_main]
#![warn(rust_2018_idioms)]
#![warn(unsafe_op_in_unsafe_fn)]
#![allow(unstable_name_collisions)]
#![allow(clippy::missing_safety_doc)]

use ::log::info;
use hermit_entry::boot_info::{BootInfo, RawBootInfo};

#[macro_use]
mod macros;

mod arch;
mod bump_allocator;
#[cfg(any(target_os = "uefi", target_arch = "x86_64"))]
mod fdt;
mod log;
mod os;

#[cfg(any(target_os = "uefi", all(target_arch = "x86_64", target_os = "none")))]
extern crate alloc;

mod built_info {
	include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

fn log_built_info() {
	let version = env!("CARGO_PKG_VERSION");
	info!("Hermit Loader version {version}");
	if let Some(git_version) = built_info::GIT_VERSION {
		let dirty = if built_info::GIT_DIRTY == Some(true) {
			" (dirty)"
		} else {
			""
		};

		let opt_level = if built_info::OPT_LEVEL == "3" {
			format_args!("")
		} else {
			format_args!(" (opt-level={})", built_info::OPT_LEVEL)
		};

		info!("Git version: {git_version}{dirty}{opt_level}");
	}
	let arch = built_info::TARGET.split_once('-').unwrap().0;
	info!("Architecture: {arch}");
	info!("Enabled features: {}", built_info::FEATURES_LOWERCASE_STR);
	info!("Built on {}", built_info::BUILT_TIME_UTC);
}

trait BootInfoExt {
	fn write(self) -> &'static RawBootInfo;
}

impl BootInfoExt for BootInfo {
	fn write(self) -> &'static RawBootInfo {
		info!("boot_info = {self:#x?}");

		take_static::take_static! {
			static RAW_BOOT_INFO: Option<RawBootInfo> = None;
		}

		let raw_boot_info = RAW_BOOT_INFO.take().unwrap();

		raw_boot_info.insert(RawBootInfo::from(self))
	}
}

#[doc(hidden)]
fn _print(args: core::fmt::Arguments<'_>) {
	use core::fmt::Write;

	self::os::CONSOLE.lock().write_fmt(args).unwrap();
}
