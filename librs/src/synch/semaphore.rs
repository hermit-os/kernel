// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//               2018 Colin Finck, RWTH Aachen University
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
use scheduler::task::{PriorityTaskQueue, WakeupReason};
use synch::spinlock::SpinlockIrqSave;


struct SemaphoreState {
	/// Resource available count
	count: isize,
	/// Priority queue of waiting tasks
	queue: PriorityTaskQueue,
}

/// A counting, blocking, semaphore.
///
/// Semaphores are a form of atomic counter where access is only granted if the
/// counter is a positive value. Each acquisition will block the calling thread
/// until the counter is positive, and each release will increment the counter
/// and unblock any threads if necessary.
///
/// # Examples
///
/// ```
///
/// // Create a semaphore that represents 5 resources
/// let sem = Semaphore::new(5);
///
/// // Acquire one of the resources
/// sem.acquire();
///
/// // Acquire one of the resources for a limited period of time
/// {
///     let _guard = sem.access();
///     // ...
/// } // resources is released here
///
/// // Release our initially acquired resource
/// sem.release();
///
/// Interface is derived from https://doc.rust-lang.org/1.7.0/src/std/sync/semaphore.rs.html
/// ```
pub struct Semaphore {
	state: SpinlockIrqSave<SemaphoreState>
}

impl Semaphore {
	/// Creates a new semaphore with the initial count specified.
	///
	/// The count specified can be thought of as a number of resources, and a
	/// call to `acquire` or `access` will block until at least one resource is
	/// available. It is valid to initialize a semaphore with a negative count.
	pub fn new(count: isize) -> Self {
		Self {
			state: SpinlockIrqSave::new(SemaphoreState {
				count: count,
				queue: PriorityTaskQueue::new(),
			}),
		}
	}

	/// Acquires a resource of this semaphore, blocking the current thread until
	/// it can do so or until the wakeup time has elapsed.
	///
	/// This method will block until the internal count of the semaphore is at
	/// least 1.
	pub fn acquire(&self, wakeup_time: Option<usize>) -> bool {
		// Reset last_wakeup_reason and get the priority of the current task.
		let core_scheduler = core_scheduler();
		let prio = {
			let mut borrowed = core_scheduler.current_task.borrow_mut();
			borrowed.last_wakeup_reason = WakeupReason::Custom;
			borrowed.prio
		};

		// Loop until we have acquired the semaphore.
		loop {
			{
				let mut locked_state = self.state.lock();

				if locked_state.count > 0 {
					// Successfully acquired the semaphore.
					locked_state.count -= 1;
					return true;
				} else if core_scheduler.current_task.borrow().last_wakeup_reason == WakeupReason::Timer {
					// We could not acquire the semaphore and we were woken up because the wakeup time has elapsed.
					// Don't try again and return the failure status.
					return false;
				}

				// We couldn't acquire the semaphore.
				// Block the current task and add it to the wakeup queue.
				core_scheduler.blocked_tasks.lock().add(core_scheduler.current_task.clone(), wakeup_time);
				locked_state.queue.push(prio, core_scheduler.current_task.clone());
			}

			// Switch to the next task.
			core_scheduler.scheduler();
		}
	}

	pub fn try_acquire(&self) -> bool {
		let mut locked_state = self.state.lock();

		if locked_state.count > 0 {
			locked_state.count -= 1;
			true
		} else {
			false
		}
	}

	/// Release a resource from this semaphore.
	///
	/// This will increment the number of resources in this semaphore by 1 and
	/// will notify any pending waiters in `acquire` or `access` if necessary.
	pub fn release(&self) {
		let mut locked_state = self.state.lock();
		locked_state.count += 1;

		// Wake up any task that has been waiting for this semaphore.
		if let Some(task) = locked_state.queue.pop() {
			let core_scheduler = scheduler::get_scheduler(task.borrow().core_id);
			core_scheduler.blocked_tasks.lock().custom_wakeup(task);
		}
	}
}
