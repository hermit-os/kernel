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
mod log;
mod os;

#[cfg(any(
	target_os = "uefi",
	all(target_arch = "x86_64", target_os = "none", not(feature = "fc"))
))]
extern crate alloc;

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
