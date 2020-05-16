// Copyright (c) 2019 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use alloc::boxed::Box;
use arch::percore::*;
use core::mem;
use scheduler::task::TaskHandlePriorityQueue;

struct CondQueue {
	queue: TaskHandlePriorityQueue,
}

impl CondQueue {
	pub fn new() -> Self {
		CondQueue {
			queue: TaskHandlePriorityQueue::new(),
		}
	}
}

unsafe fn __sys_destroy_queue(ptr: usize) -> i32 {
	let id = ptr as *mut usize;
	if id.is_null() {
		debug!("sys_wait: ivalid address to condition variable");
		return -1;
	}

	if *id != 0 {
		let cond = Box::from_raw((*id) as *mut CondQueue);
		mem::drop(cond);
	}

	0
}

#[no_mangle]
pub unsafe fn sys_destroy_queue(ptr: usize) -> i32 {
	kernel_function!(__sys_destroy_queue(ptr))
}

unsafe fn __sys_notify(ptr: usize, count: i32) -> i32 {
	let id = ptr as *const usize;

	if id.is_null() {
		// invalid argument
		debug!("sys_notify: invalid address to condition variable");
		return -1;
	}

	if *id == 0 {
		debug!("sys_notify: invalid reference to condition variable");
		return -1;
	}

	let cond = &mut *((*id) as *mut CondQueue);

	if count < 0 {
		// Wake up all task that has been waiting for this condition variable
		while let Some(task) = cond.queue.pop() {
			core_scheduler().custom_wakeup(task);
		}
	} else {
		for _ in 0..count {
			// Wake up any task that has been waiting for this condition variable
			if let Some(task) = cond.queue.pop() {
				core_scheduler().custom_wakeup(task);
			} else {
				debug!("Unable to wakeup task");
			}
		}
	}

	0
}

#[no_mangle]
pub unsafe fn sys_notify(ptr: usize, count: i32) -> i32 {
	kernel_function!(__sys_notify(ptr, count))
}

unsafe fn __sys_init_queue(ptr: usize) -> i32 {
	let id = ptr as *mut usize;
	if id.is_null() {
		debug!("sys_init_queue: ivalid address to condition variable");
		return -1;
	}

	if *id == 0 {
		debug!("Create condition variable queue");
		let queue = Box::new(CondQueue::new());
		*id = Box::into_raw(queue) as usize;
	}

	0
}

#[no_mangle]
pub unsafe fn sys_init_queue(ptr: usize) -> i32 {
	kernel_function!(__sys_init_queue(ptr))
}

unsafe fn __sys_add_queue(ptr: usize, timeout_ns: i64) -> i32 {
	let id = ptr as *mut usize;
	if id.is_null() {
		debug!("sys_add_queue: invalid address to condition variable");
		return -1;
	}

	if *id == 0 {
		debug!("Create condition variable queue");
		let queue = Box::new(CondQueue::new());
		*id = Box::into_raw(queue) as usize;
	}

	let wakeup_time = if timeout_ns <= 0 {
		None
	} else {
		Some(timeout_ns as u64 / 1000)
	};

	// Block the current task and add it to the wakeup queue.
	let core_scheduler = core_scheduler();
	core_scheduler.block_current_task(wakeup_time);
	let cond = &mut *((*id) as *mut CondQueue);
	cond.queue.push(core_scheduler.get_current_task_handle());

	0
}

#[no_mangle]
pub unsafe fn sys_add_queue(ptr: usize, timeout_ns: i64) -> i32 {
	kernel_function!(__sys_add_queue(ptr, timeout_ns))
}

fn __sys_wait(_ptr: usize) -> i32 {
	// Switch to the next task.
	core_scheduler().reschedule();

	0
}

#[no_mangle]
pub fn sys_wait(ptr: usize) -> i32 {
	kernel_function!(__sys_wait(ptr))
}
