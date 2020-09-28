#![feature(test)]
#![no_std]
#![no_main]
#![test_runner(common::test_case_runner)]
#![feature(custom_test_frameworks)]
#![reexport_test_harness_main = "test_main"]

extern crate hermit;

#[macro_use]
use common::*;
mod common;

#[macro_use]
use alloc::vec;
use hermit::{sys_join, sys_spawn2, sys_usleep, USER_STACK_SIZE};

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

#[no_mangle]
extern "C" fn runtime_entry(_argc: i32, _argv: *const *const u8, _env: *const *const u8) -> ! {
	test_main();
	common::exit(false)
}
