#![no_std]
#![no_main]
#![test_runner(common::test_case_runner)]
#![feature(custom_test_frameworks)]
#![reexport_test_harness_main = "test_main"]

mod common;

use hermit::fd::{EventFlags, PollEvent, PollFd};
use hermit::syscalls::{
	sys_clock_gettime, sys_close, sys_eventfd, sys_join, sys_poll, sys_spawn2, sys_usleep,
	sys_write,
};
use hermit::time::timespec;

const CLOCK_MONOTONIC: i32 = 4;
const NORMAL_PRIO: u8 = 2;
const USER_STACK_SIZE: usize = 0x0010_0000;

fn now_ms() -> i64 {
	let mut ts = timespec::default();
	let ret = unsafe { sys_clock_gettime(CLOCK_MONOTONIC, &raw mut ts) };
	assert_eq!(ret, 0);
	ts.tv_sec * 1000 + i64::from(ts.tv_nsec) / 1_000_000
}

/// poll() on a descriptor that never becomes ready must return 0 once its
/// timeout expires, instead of blocking until an event arrives.
#[test_case]
fn poll_times_out() {
	let fd = sys_eventfd(0, EventFlags::empty().bits());
	assert!(fd >= 0);
	let mut fds = [PollFd {
		fd,
		events: PollEvent::POLLIN,
		revents: PollEvent::empty(),
	}];

	let start = now_ms();
	let ret = unsafe { sys_poll(fds.as_mut_ptr(), fds.len(), 100) };
	let elapsed = now_ms() - start;

	assert_eq!(ret, 0);
	assert!(fds[0].revents.is_empty());
	assert!(
		(100..5_000).contains(&elapsed),
		"poll with a 100 ms timeout returned after {elapsed} ms"
	);

	sys_close(fd);
}

extern "C" fn write_after_50ms(fd: usize) {
	sys_usleep(50_000);
	let one = 1u64.to_ne_bytes();
	let ret = unsafe { sys_write(fd as i32, one.as_ptr(), one.len()) };
	assert_eq!(ret, 8);
}

/// An event that arrives before the timeout must wake poll() early.
#[test_case]
fn poll_wakes_on_event_before_timeout() {
	let fd = sys_eventfd(0, EventFlags::empty().bits());
	assert!(fd >= 0);
	let mut fds = [PollFd {
		fd,
		events: PollEvent::POLLIN,
		revents: PollEvent::empty(),
	}];

	let start = now_ms();
	let writer = unsafe {
		sys_spawn2(
			write_after_50ms,
			fd as usize,
			NORMAL_PRIO,
			USER_STACK_SIZE,
			-1,
		)
	};
	assert!(writer >= 0);
	let ret = unsafe { sys_poll(fds.as_mut_ptr(), fds.len(), 5_000) };
	let elapsed = now_ms() - start;
	sys_join(writer);

	assert_eq!(ret, 1);
	assert!(fds[0].revents.contains(PollEvent::POLLIN));
	assert!(
		elapsed < 2_500,
		"poll woken by an event after 50 ms returned after {elapsed} ms"
	);

	sys_close(fd);
}

#[unsafe(no_mangle)]
extern "C" fn runtime_entry(_argc: i32, _argv: *const *const u8, _env: *const *const u8) -> ! {
	test_main();
	common::exit(false)
}
