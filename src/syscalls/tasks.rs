// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use arch;
use arch::percore::*;
use core::isize;
use errno::*;
use scheduler;
use scheduler::task::{TaskId, Priority};
use syscalls::timer::timespec;

pub type SignalHandler = extern "C" fn(i32);
pub type Tid = u32;


#[no_mangle]
pub extern "C" fn sys_getpid() -> Tid {
	let current_task_borrowed = core_scheduler().current_task.borrow();
	current_task_borrowed.id.into() as Tid
}

#[no_mangle]
pub extern "C" fn sys_getprio(id: *const Tid) -> i32 {
	let current_task_borrowed = core_scheduler().current_task.borrow();

	if id.is_null() || unsafe {*id} == current_task_borrowed.id.into() as u32 {
		current_task_borrowed.prio.into() as i32
	} else {
		-EINVAL
	}
}

#[no_mangle]
pub extern "C" fn sys_setprio(_id: *const Tid, _prio: i32) -> i32 {
	-ENOSYS
}

#[no_mangle]
pub extern "C" fn sys_exit(arg: i32) -> ! {
	core_scheduler().exit(arg);
}

#[no_mangle]
pub extern "C" fn sys_abort() -> ! {
    sys_exit(-1);
}

#[no_mangle]
pub extern "C" fn sys_sbrk(incr: isize) -> usize {
	// Get the boundaries of the task heap and verify that they are suitable for sbrk.
	let task_heap_start = arch::mm::virtualmem::task_heap_start();
	let task_heap_end = arch::mm::virtualmem::task_heap_end();
	assert!(task_heap_end <= isize::MAX as usize);

	// Get the heap of the current task on the current core.
	let mut current_task_borrowed = core_scheduler().current_task.borrow_mut();
	let heap = current_task_borrowed.heap.as_mut().expect("Calling sys_sbrk on a task without an associated heap");

	// Adjust the heap of the current task.
	let heap_borrowed = heap.borrow();
	let mut heap_locked = heap_borrowed.write();
	assert!(heap_locked.start >= task_heap_start, "heap start {:#X} is not >= task_heap_start {:#X}", heap_locked.start, task_heap_start);
	let old_end = heap_locked.end;
	heap_locked.end = (old_end as isize + incr) as usize;
	assert!(heap_locked.end <= task_heap_end, "New heap end {:#X} is not <= task_heap_end {:#X}", heap_locked.end, task_heap_end);

	debug!("Adjusted task heap from {:#X} to {:#X}", old_end, heap_locked.end);

	// We're done! The page fault handler will map the new virtual memory area to physical memory
	// as soon as the task accesses it for the first time.
	old_end
}

#[no_mangle]
pub extern "C" fn sys_usleep(usecs: u64) {
	if usecs > (scheduler::TASK_TIME_SLICE as u64) {
		// Enough time to set a wakeup timer and block the current task.
		debug!("sys_usleep blocking the task for {} microseconds", usecs);
		let wakeup_time = arch::processor::get_timer_ticks() + usecs;
		let core_scheduler = core_scheduler();
		let current_task = core_scheduler.current_task.clone();
		core_scheduler.blocked_tasks.lock().add(current_task, Some(wakeup_time));

		// Switch to the next task.
		core_scheduler.scheduler();
	} else if usecs > 0 {
		// Not enough time to set a wakeup timer, so just do busy-waiting.
		arch::processor::udelay(usecs);
	}
}

#[no_mangle]
pub extern "C" fn sys_msleep(ms: u32) {
	sys_usleep((ms as u64) * 1000);
}

#[no_mangle]
pub extern "C" fn sys_nanosleep(rqtp: *const timespec, _rmtp: *mut timespec) -> i32 {
	assert!(!rqtp.is_null(), "sys_nanosleep called with a zero rqtp parameter");
	let requested_time = unsafe { & *rqtp };
	if requested_time.tv_sec < 0 || requested_time.tv_nsec < 0 || requested_time.tv_nsec > 999_999_999 {
		debug!("sys_nanosleep called with an invalid requested time, returning -EINVAL");
		return -EINVAL;
	}

	let microseconds = (requested_time.tv_sec as u64) * 1_000_000 + (requested_time.tv_nsec as u64) / 1_000;
	sys_usleep(microseconds);
	0
}

#[no_mangle]
pub extern "C" fn sys_clone(id: *mut Tid, func: extern "C" fn(usize), arg: usize) -> i32 {
	let task_id = core_scheduler().clone(func, arg);

	if !id.is_null() {
		unsafe { *id = task_id.into() as u32; }
	}

	0
}

#[no_mangle]
pub extern "C" fn sys_yield() {
	core_scheduler().scheduler();
}

#[no_mangle]
pub extern "C" fn sys_kill(dest: Tid, signum: i32) -> i32 {
	debug!("sys_kill is unimplemented, returning -ENOSYS for killing {} with signal {}", dest, signum);
	-ENOSYS
}

#[no_mangle]
pub extern "C" fn sys_signal(_handler: SignalHandler) -> i32 {
	debug!("sys_signal is unimplemented");
	0
}

#[no_mangle]
pub extern "C" fn sys_spawn(id: *mut Tid, func: extern "C" fn(usize), arg: usize, prio: u8, core_id: usize) -> i32 {
	let core_scheduler = scheduler::get_scheduler(core_id);
	let task_id = core_scheduler.spawn(func, arg, Priority::from(prio), None);

	if !id.is_null() {
		unsafe { *id = task_id.into() as u32; }
	}

	0
}

#[no_mangle]
pub extern "C" fn sys_join(id: Tid) -> i32 {
	match scheduler::join(TaskId::from(id)) {
		Ok(()) => 0,
		_ => -EINVAL
	}
}
