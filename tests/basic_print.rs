#![no_std]
#![no_main]
//#![test_runner(hermit::test_runner)]
//#![feature(custom_test_frameworks)]
//#![reexport_test_harness_main = "test_main"]

//use core::panic::PanicInfo;
extern crate hermit;

mod common;
use common::*;

/// Print all Strings the application got passed as arguments
#[no_mangle]
pub fn main(args: Vec<String>) {
	for s in args {
		println!("{}", &s);
	}
}
