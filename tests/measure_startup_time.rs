#![no_std]
#![no_main]

extern crate alloc;

#[macro_use]
extern crate hermit;

mod common;

use alloc::string::String;
use alloc::vec::Vec;

/// This Test lets the runner measure the basic overhead of the tests including
/// - hypervisor startup time
/// - kernel boot-time
/// - overhead of runtime_entry (test entry)
#[no_mangle]
pub fn main(_args: Vec<String>) -> Result<(), ()> {
	Ok(())
}

runtime_entry_with_args!();
