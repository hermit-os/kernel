#![no_std]
#![no_main]
//#![test_runner(hermit::test_runner)]
//#![feature(custom_test_frameworks)]
//#![reexport_test_harness_main = "test_main"]

//use core::panic::PanicInfo;
extern crate hermit;
use hermit::{print, println};

//ToDo: Define exit code enum in hermit!!!

// Workaround since the "real" runtime_entry function (defined in libstd) is not available,
// since the target-os is hermit-kernel and not hermit
#[no_mangle]
extern "C" fn runtime_entry(argc: i32, argv: *const *const u8, env: *const *const u8) -> ! {
	main(argc as isize, argv);
	hermit::sys_exit(-1);
}

//#[test_case]
pub fn main(argc: isize, argv: *const *const u8) {
	println!("hey we made it to the test function :O");
}
