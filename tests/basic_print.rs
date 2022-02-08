#![feature(test)]
#![no_std]
#![no_main]
//#![test_runner(hermit::test_runner)]
//#![feature(custom_test_frameworks)]
//#![reexport_test_harness_main = "test_main"]

//use core::panic::PanicInfo;
extern crate hermit;

use common::*;
mod common;

/// Print all Strings the application got passed as arguments
#[no_mangle]
pub fn main(args: Vec<String>) -> Result<(), ()> {
	for s in args {
		println!("{}", &s);
	}
	Ok(()) // real assertion is done by the runner
}

runtime_entry_with_args!();
