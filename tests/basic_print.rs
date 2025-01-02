#![no_std]
#![no_main]

extern crate alloc;

#[macro_use]
extern crate hermit;

mod common;

use alloc::string::String;
use alloc::vec::Vec;

/// Print all Strings the application got passed as arguments
#[unsafe(no_mangle)]
pub fn main(args: Vec<String>) -> Result<(), String> {
	for s in args {
		println!("{}", &s);
	}
	Ok(()) // real assertion is done by the runner
}

runtime_entry_with_args!();
