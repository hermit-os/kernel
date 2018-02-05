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


pub struct RecursiveMutexData {
	current_tid: Option<TaskId>,
	counter: usize,
}

pub struct RecursiveMutex {
	data: Spinlock<RecursiveMutexData>,
	queue: Spinlock<PriorityTaskQueue>,
}

impl RecursiveMutex {
	pub fn new() -> Self {
		Self {
			data: Spinlock::new(RecursiveMutexData { current_tid: None, counter: 0 }),
			queue: Spinlock::new(PriorityTaskQueue::new()),
		}
	}

	pub fn acquire(&self) {
		// Get the current task ID.
		let core_scheduler = scheduler::get_scheduler(core_id());
		let current_task = core_scheduler.get_current_task();
		let tid = current_task.borrow().id;

		loop {
			{
				// Lock the entire RecursiveMutexData structure within this scope, so all its fields are
				// modified in one atomic operation.
				let mut lock = self.data.lock();

				// Is the mutex currently acquired?
				if let Some(current_tid) = lock.current_tid {
					// Has it been acquired by the same task?
					if current_tid == tid {
						// Yes, so just increment the counter (recursive mutex behavior).
						lock.counter += 1;
						return;
					}
				} else {
					// The mutex is currently not acquired, so we become its new owner.
					lock.current_tid = Some(tid);
					lock.counter = 1;
					return;
				}
			}

			// The mutex is currently acquired by another task.
			// Block the current task and add it to the wakeup queue.
			core_scheduler.blocked_tasks.lock().add(current_task.clone(), None);
			self.queue.lock().push(current_task.borrow().prio, current_task.clone());

			// Switch to the next task.
			core_scheduler.scheduler();
		}
	}

	pub fn release(&self) {
		// Get the current task ID.
		let tid = {
			let core_scheduler = scheduler::get_scheduler(core_id());
			let current_task = core_scheduler.get_current_task();
			let borrowed = current_task.borrow();
			borrowed.id
		};

		// Lock the entire RecursiveMutexData structure and do some sanity checks.
		let mut lock = self.data.lock();
		let current_tid = lock.current_tid.expect("Trying to release a RecursiveMutex which has not been acquired");
		assert!(current_tid == tid);

		// Decrement the counter (recursive mutex behavior).
		lock.counter -= 1;
		if lock.counter == 0 {
			// Release the entire recursive mutex and drop the spinlock before waking up tasks.
			lock.current_tid = None;
			drop(lock);

			// Wake up a task that has been waiting for this mutex.
			if let Some(task) = self.queue.lock().pop() {
				let core_scheduler = scheduler::get_scheduler(task.borrow().core_id);
				core_scheduler.blocked_tasks.lock().custom_wakeup(task);
			}
		}
	}
}
