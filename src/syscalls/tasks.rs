// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use arch;
use arch::kernel::get_processor_count;
use arch::percore::*;
use core::isize;
use core::sync::atomic::{AtomicUsize, Ordering};
use errno::*;
#[cfg(feature = "newlib")]
use mm::{task_heap_end, task_heap_start};
use scheduler;
use scheduler::task::{Priority, TaskId};
use syscalls;
use syscalls::timer::timespec;

#[cfg(feature = "newlib")]
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

	if id.is_null() || unsafe { *id } == current_task_borrowed.id.into() as u32 {
		i32::from(current_task_borrowed.prio.into())
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
	debug!("Exit program with error code {}!", arg);
	syscalls::sys_shutdown(arg);
}

#[no_mangle]
pub extern "C" fn sys_thread_exit(arg: i32) -> ! {
	debug!("Exit thread with error code {}!", arg);
	core_scheduler().exit(arg);
}

#[no_mangle]
pub extern "C" fn sys_abort() -> ! {
	sys_exit(-1);
}

#[cfg(feature = "newlib")]
static SBRK_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "newlib")]
pub fn sbrk_init() {
	SBRK_COUNTER.store(task_heap_start(), Ordering::SeqCst);
}

#[cfg(feature = "newlib")]
#[no_mangle]
pub extern "C" fn sys_sbrk(incr: isize) -> usize {
	// Get the boundaries of the task heap and verify that they are suitable for sbrk.
	let task_heap_start = task_heap_start();
	let task_heap_end = task_heap_end();
	let old_end;

	if incr >= 0 {
		old_end = SBRK_COUNTER.fetch_add(incr as usize, Ordering::SeqCst);
		assert!(task_heap_end >= old_end + incr as usize);
	} else {
		old_end = SBRK_COUNTER.fetch_sub(incr.abs() as usize, Ordering::SeqCst);
		assert!(task_heap_start < old_end - incr.abs() as usize);
	}

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
		core_scheduler
			.blocked_tasks
			.lock()
			.add(current_task, Some(wakeup_time));

		// Switch to the next task.
		core_scheduler.scheduler();
	} else if usecs > 0 {
		// Not enough time to set a wakeup timer, so just do busy-waiting.
		arch::processor::udelay(usecs);
	}
}

#[no_mangle]
pub extern "C" fn sys_msleep(ms: u32) {
	sys_usleep(u64::from(ms) * 1000);
}

#[no_mangle]
pub extern "C" fn sys_nanosleep(rqtp: *const timespec, _rmtp: *mut timespec) -> i32 {
	assert!(
		!rqtp.is_null(),
		"sys_nanosleep called with a zero rqtp parameter"
	);
	let requested_time = unsafe { &*rqtp };
	if requested_time.tv_sec < 0
		|| requested_time.tv_nsec < 0
		|| requested_time.tv_nsec > 999_999_999
	{
		debug!("sys_nanosleep called with an invalid requested time, returning -EINVAL");
		return -EINVAL;
	}

	let microseconds =
		(requested_time.tv_sec as u64) * 1_000_000 + (requested_time.tv_nsec as u64) / 1_000;
	sys_usleep(microseconds);

	0
}

#[cfg(feature = "newlib")]
#[no_mangle]
pub extern "C" fn sys_clone(id: *mut Tid, func: extern "C" fn(usize), arg: usize) -> i32 {
	let task_id = core_scheduler().clone(func, arg);

	if !id.is_null() {
		unsafe {
			*id = task_id.into() as u32;
		}
	}

	0
}

#[no_mangle]
pub extern "C" fn sys_yield() {
	core_scheduler().scheduler();
}

#[cfg(feature = "newlib")]
#[no_mangle]
pub extern "C" fn sys_kill(dest: Tid, signum: i32) -> i32 {
	debug!(
		"sys_kill is unimplemented, returning -ENOSYS for killing {} with signal {}",
		dest, signum
	);
	-ENOSYS
}

#[cfg(feature = "newlib")]
#[no_mangle]
pub extern "C" fn sys_signal(_handler: SignalHandler) -> i32 {
	debug!("sys_signal is unimplemented");
	0
}

#[no_mangle]
pub extern "C" fn sys_spawn(
	id: *mut Tid,
	func: extern "C" fn(usize),
	arg: usize,
	prio: u8,
	selector: isize,
) -> i32 {
	static CORE_COUNTER: AtomicUsize = AtomicUsize::new(0);

	let core_id = if selector < 0 {
		// use Round Robin to schedule the cores
		CORE_COUNTER.fetch_add(1, Ordering::SeqCst) % get_processor_count()
	} else {
		selector as usize
	};

	let core_scheduler = scheduler::get_scheduler(core_id);
	let task_id = core_scheduler.spawn(func, arg, Priority::from(prio));

	if !id.is_null() {
		unsafe {
			*id = task_id.into() as u32;
		}
	}

	0
}

#[no_mangle]
pub extern "C" fn sys_join(id: Tid) -> i32 {
	match scheduler::join(TaskId::from(id)) {
		Ok(()) => 0,
		_ => -EINVAL,
	}
}
