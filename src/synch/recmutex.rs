// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::arch::percore::*;
use crate::scheduler::task::{TaskHandlePriorityQueue, TaskId};
use crate::synch::spinlock::Spinlock;

struct RecursiveMutexState {
	current_tid: Option<TaskId>,
	count: usize,
	queue: TaskHandlePriorityQueue,
}

pub struct RecursiveMutex {
	state: Spinlock<RecursiveMutexState>,
}

impl RecursiveMutex {
	pub const fn new() -> Self {
		Self {
			state: Spinlock::new(RecursiveMutexState {
				current_tid: None,
				count: 0,
				queue: TaskHandlePriorityQueue::new(),
			}),
		}
	}

	pub fn acquire(&self) {
		// Get information about the current task.
		let core_scheduler = core_scheduler();
		let tid = core_scheduler.get_current_task_id();

		loop {
			{
				let mut locked_state = self.state.lock();

				// Is the mutex currently acquired?
				if let Some(current_tid) = locked_state.current_tid {
					// Has it been acquired by the same task?
					if current_tid == tid {
						// Yes, so just increment the counter (recursive mutex behavior).
						locked_state.count += 1;
						return;
					}
				} else {
					// The mutex is currently not acquired, so we become its new owner.
					locked_state.current_tid = Some(tid);
					locked_state.count = 1;
					return;
				}

				// The mutex is currently acquired by another task.
				// Block the current task and add it to the wakeup queue.
				core_scheduler.block_current_task(None);
				locked_state
					.queue
					.push(core_scheduler.get_current_task_handle());
			}

			// Switch to the next task.
			core_scheduler.reschedule();
		}
	}

	pub fn release(&self) {
		if let Some(task) = {
			let mut locked_state = self.state.lock();

			// We could do a sanity check here whether the RecursiveMutex is actually held by the current task.
			// But let's just trust our code using this function for the sake of simplicity and performance.

			// Decrement the counter (recursive mutex behavior).
			locked_state.count -= 1;
			if locked_state.count == 0 {
				// Release the entire recursive mutex.
				locked_state.current_tid = None;

				locked_state.queue.pop()
			} else {
				None
			}
		} {
			// Wake up any task that has been waiting for this mutex.
			core_scheduler().custom_wakeup(task);
		}
	}
}

// Same unsafe impls as `RecursiveMutex`
unsafe impl Sync for RecursiveMutex {}
unsafe impl Send for RecursiveMutex {}
