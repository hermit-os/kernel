// Copyright (c) 2019 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use alloc::boxed::Box;
use arch::percore::*;
use core::mem;
use scheduler;
use scheduler::task::PriorityTaskQueue;

struct CondQueue {
	queue: PriorityTaskQueue,
	id: usize,
}

impl CondQueue {
	pub fn new(id: usize) -> Self {
		CondQueue {
			queue: PriorityTaskQueue::new(),
			id: id,
		}
	}
}

impl Drop for CondQueue {
	fn drop(&mut self) {
		debug!("Drop queue for condition variable with id 0x{:x}", self.id);
	}
}

#[no_mangle]
pub unsafe fn sys_destroy_queue(ptr: usize) -> i32 {
	let id = ptr as *mut usize;
	if id.is_null() {
		debug!("sys_wait: ivalid address to condition variable");
		return -1;
	}

	if *id != 0 {
		let cond = Box::from_raw((*id) as *mut CondQueue);
		mem::drop(cond);

		// reset id
		*id = 0;
	}

	0
}

#[no_mangle]
pub unsafe fn sys_notify(ptr: usize, count: i32) -> i32 {
	let id = ptr as *const usize;
	if id.is_null() || *id == 0 {
		// invalid argument
		debug!("sys_notify: invalid address to condition variable");
		return -1;
	}

	let cond = &mut *((*id) as *mut CondQueue);

	if count < 0 {
		// Wake up all task that has been waiting for this condition variable
		while let Some(task) = cond.queue.pop() {
			let core_scheduler = scheduler::get_scheduler(task.borrow().core_id);
			core_scheduler.blocked_tasks.lock().custom_wakeup(task);
		}
	} else {
		for _ in 0..count {
			// Wake up any task that has been waiting for this condition variable
			if let Some(task) = cond.queue.pop() {
				let core_scheduler = scheduler::get_scheduler(task.borrow().core_id);
				core_scheduler.blocked_tasks.lock().custom_wakeup(task);
			} else {
				debug!("Unable to wakeup task");
			}
		}
	}

	0
}

#[no_mangle]
pub unsafe fn sys_add_queue(ptr: usize, timeout_ns: i64) -> i32 {
	let id = ptr as *mut usize;
	if id.is_null() {
		debug!("sys_wait: ivalid address to condition variable");
		return -1;
	}

	if *id == 0 {
		debug!("Create condition variable queue");
		let queue = Box::new(CondQueue::new(ptr));
		*id = Box::into_raw(queue) as usize;
	}

	let wakeup_time = if timeout_ns <= 0 {
		None
	} else {
		Some(timeout_ns as u64 / 1000)
	};

	// Block the current task and add it to the wakeup queue.
	let core_scheduler = core_scheduler();
	core_scheduler
		.blocked_tasks
		.lock()
		.add(core_scheduler.current_task.clone(), wakeup_time);

	{
		let cond = &mut *((*id) as *mut CondQueue);
		cond.queue.push(core_scheduler.current_task.clone());
	}

	0
}

#[no_mangle]
pub fn sys_wait(_ptr: usize) -> i32 {
	// Switch to the next task.
	core_scheduler().reschedule();

	0
}
