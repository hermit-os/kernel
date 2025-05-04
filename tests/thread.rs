#![feature(test)]
#![feature(thread_local)]
#![no_std]
#![no_main]
#![test_runner(common::test_case_runner)]
#![feature(custom_test_frameworks)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

#[macro_use]
extern crate hermit;

use core::ptr;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering::Relaxed;

mod common;

use alloc::vec;

use hermit::errno::{EAGAIN, ETIMEDOUT};
use hermit::syscalls::{sys_futex_wait, sys_futex_wake, sys_join, sys_spawn2, sys_usleep};
use hermit::time::timespec;

const USER_STACK_SIZE: usize = 0x0010_0000;
const NORMAL_PRIO: u8 = 2;

extern "C" fn thread_func(i: usize) {
	println!("this is thread number {}", i);
	sys_usleep(2_000_000);
	println!("---------------THREAD DONE!---------- {}", i);
}

#[test_case]
pub fn thread_test() {
	let mut children = vec![];

	let threadnum = 5;
	for i in 0..threadnum {
		println!("SPAWNING THREAD {}", i);
		let id = unsafe { sys_spawn2(thread_func, i, NORMAL_PRIO, USER_STACK_SIZE, -1) };
		children.push(id);
	}
	println!("SPAWNED THREADS");

	for child in children {
		sys_join(child);
	}
}

unsafe extern "C" fn waker_func(futex: usize) {
	let futex = unsafe { &*(futex as *const AtomicU32) };

	sys_usleep(100_000);

	futex.store(1, Relaxed);
	let ret = unsafe { sys_futex_wake(futex.as_ptr(), i32::MAX) };
	assert_eq!(ret, 1);
}

#[test_case]
pub fn test_futex() {
	let futex = AtomicU32::new(0);
	let futex_ptr = futex.as_ptr();

	let ret = unsafe { sys_futex_wait(futex_ptr, 1, ptr::null(), 0) };
	assert_eq!(ret, -EAGAIN);

	let timeout = timespec {
		tv_sec: 0,
		tv_nsec: 100_000_000,
	};
	let ret = unsafe { sys_futex_wait(futex_ptr, 0, &raw const timeout, 1) };
	assert_eq!(ret, -ETIMEDOUT);

	let waker = unsafe {
		sys_spawn2(
			waker_func,
			futex_ptr as usize,
			NORMAL_PRIO,
			USER_STACK_SIZE,
			-1,
		)
	};
	assert!(waker >= 0);

	let ret = unsafe { sys_futex_wait(futex_ptr, 0, ptr::null(), 0) };
	assert_eq!(ret, 0);
	assert_eq!(futex.load(Relaxed), 1);

	let ret = sys_join(waker);
	assert_eq!(ret, 0);
}

#[test_case]
pub fn test_thread_local() {
	#[repr(C, align(0x10))]
	struct AlignedByte(u8);

	#[thread_local]
	static mut BYTE: u8 = 0x42;

	#[thread_local]
	static mut CAFECAFE: u64 = 0xcafe_cafe;

	#[thread_local]
	static mut DEADBEEF: u64 = 0xdead_beef;

	#[thread_local]
	static mut ALIGNED_BYTE: AlignedByte = AlignedByte(0x53);

	// If the thread local statics are not mut, they get optimized away in release.
	unsafe {
		assert_eq!(0x42, { BYTE });
		assert_eq!(0xcafe_cafe, { CAFECAFE });
		assert_eq!(0xdead_beef, { DEADBEEF });
		assert_eq!(0x53, { ALIGNED_BYTE.0 });
	}
}

#[unsafe(no_mangle)]
extern "C" fn runtime_entry(_argc: i32, _argv: *const *const u8, _env: *const *const u8) -> ! {
	test_main();
	common::exit(false)
}
