#![no_std]
#![no_main]

extern crate hermit;

mod common;
use common::*;

/// This Test lets the runner measure the basic overhead of the tests including
/// - hypervisor startup time
/// - kernel boot-time
/// - overhead of runtime_entry (test entry)
#[no_mangle]
pub fn main(args: Vec<String>) -> Result<(), ()> {
	Ok(())
}
