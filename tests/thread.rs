#![feature(test)]
#![feature(thread_local)]
#![no_std]
#![no_main]
#![test_runner(common::test_case_runner)]
#![feature(custom_test_frameworks)]
#![reexport_test_harness_main = "test_main"]

extern crate hermit;

use common::*;
mod common;

use alloc::vec;
use hermit::{sys_join, sys_spawn2, sys_usleep};

const USER_STACK_SIZE: usize = 1_048_576;
const NORMAL_PRIO: u8 = 2;

extern "C" fn thread_func(i: usize) {
	println!("this is thread number {}", i);
	sys_usleep(2000000);
	println!("---------------THREAD DONE!---------- {}", i);
}

#[test_case]
pub fn thread_test() {
	let mut children = vec![];

	let threadnum = 5;
	for i in 0..threadnum {
		println!("SPAWNING THREAD {}", i);
		let id = sys_spawn2(thread_func, i, NORMAL_PRIO, USER_STACK_SIZE, -1);
		children.push(id);
	}
	println!("SPAWNED THREADS");

	for child in children {
		sys_join(child);
	}
}

#[test_case]
pub fn test_thread_local() {
	#[repr(C, align(0x1000))]
	struct AlignedByte(u8);

	#[thread_local]
	static mut BYTE: u8 = 0x42;

	#[thread_local]
	static mut CAFECAFE: u64 = 0xCAFECAFE;

	#[thread_local]
	static mut DEADBEEF: u64 = 0xDEADBEEF;

	#[thread_local]
	static mut ALIGNED_BYTE: AlignedByte = AlignedByte(0x53);

	// If the thread local statics are not mut, they get optimized away in release.
	unsafe {
		assert_eq!(0x42, BYTE);
		assert_eq!(0xCAFECAFE, CAFECAFE);
		assert_eq!(0xDEADBEEF, DEADBEEF);
		assert_eq!(0x53, ALIGNED_BYTE.0);
	}
}

#[no_mangle]
extern "C" fn runtime_entry(_argc: i32, _argv: *const *const u8, _env: *const *const u8) -> ! {
	test_main();
	common::exit(false)
}
