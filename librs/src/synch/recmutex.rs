// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// MIT License
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
// WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

use arch::percore::*;
use scheduler;
use scheduler::task::{PriorityTaskQueue, TaskId};
use synch::spinlock::Spinlock;


struct RecursiveMutexState {
	current_tid: Option<TaskId>,
	count: usize,
	queue: PriorityTaskQueue,
}

pub struct RecursiveMutex {
	state: Spinlock<RecursiveMutexState>,
}

impl RecursiveMutex {
	pub fn new() -> Self {
		Self {
			state: Spinlock::new(RecursiveMutexState {
				current_tid: None,
				count: 0,
				queue: PriorityTaskQueue::new(),
			}),
		}
	}

	pub fn acquire(&self) {
		// Get information about the current task.
		let core_scheduler = core_scheduler();
		let (prio, tid) = {
			let borrowed = core_scheduler.current_task.borrow();
			(borrowed.prio, borrowed.id)
		};

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
				core_scheduler.blocked_tasks.lock().add(core_scheduler.current_task.clone(), None);
				locked_state.queue.push(prio, core_scheduler.current_task.clone());
			}

			// Switch to the next task.
			core_scheduler.scheduler();
		}
	}

	pub fn release(&self) {
		let mut locked_state = self.state.lock();

		// We could do a sanity check here whether the RecursiveMutex is actually held by the current task.
		// But let's just trust our code using this function for the sake of simplicity and performance.

		// Decrement the counter (recursive mutex behavior).
		locked_state.count -= 1;
		if locked_state.count == 0 {
			// Release the entire recursive mutex.
			locked_state.current_tid = None;

			// Wake up any task that has been waiting for this mutex.
			if let Some(task) = locked_state.queue.pop() {
				let core_scheduler = scheduler::get_scheduler(task.borrow().core_id);
				core_scheduler.blocked_tasks.lock().custom_wakeup(task);
			}
		}
	}
}
