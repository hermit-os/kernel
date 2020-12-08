// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//               2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::arch::percore::*;
use crate::scheduler::task::{TaskHandlePriorityQueue, WakeupReason};
use crate::synch::spinlock::SpinlockIrqSave;
use crossbeam_utils::Backoff;

struct SemaphoreState {
	/// Resource available count
	count: isize,
	/// Priority queue of waiting tasks
	queue: TaskHandlePriorityQueue,
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
	state: SpinlockIrqSave<SemaphoreState>,
}

// Same unsafe impls as `Semaphore`
unsafe impl Sync for Semaphore {}
unsafe impl Send for Semaphore {}

impl Semaphore {
	/// Creates a new semaphore with the initial count specified.
	///
	/// The count specified can be thought of as a number of resources, and a
	/// call to `acquire` or `access` will block until at least one resource is
	/// available. It is valid to initialize a semaphore with a negative count.
	pub const fn new(count: isize) -> Self {
		Self {
			state: SpinlockIrqSave::new(SemaphoreState {
				count,
				queue: TaskHandlePriorityQueue::new(),
			}),
		}
	}

	/// Acquires a resource of this semaphore, blocking the current thread until
	/// it can do so or until the wakeup time (in ms) has elapsed.
	///
	/// This method will block until the internal count of the semaphore is at
	/// least 1.
	pub fn acquire(&self, time: Option<u64>) -> bool {
		let backoff = Backoff::new();
		// Reset last_wakeup_reason.
		let core_scheduler = core_scheduler();
		core_scheduler.set_current_task_wakeup_reason(WakeupReason::Custom);

		let wakeup_time = match time {
			Some(ms) => Some(crate::arch::processor::get_timer_ticks() + ms * 1000),
			_ => None,
		};

		// Loop until we have acquired the semaphore.
		loop {
			{
				let mut locked_state = self.state.lock();

				if locked_state.count > 0 {
					// Successfully acquired the semaphore.
					locked_state.count -= 1;
					return true;
				} else if let Some(t) = wakeup_time {
					if t < crate::arch::processor::get_timer_ticks() {
						// We could not acquire the semaphore and we were woken up because the wakeup time has elapsed.
						// Don't try again and return the failure status.
						locked_state
							.queue
							.remove(core_scheduler.get_current_task_handle());
						return false;
					}
				}

				if backoff.is_completed() {
					// We couldn't acquire the semaphore.
					// Block the current task and add it to the wakeup queue.
					core_scheduler.block_current_task(wakeup_time);
					locked_state
						.queue
						.push(core_scheduler.get_current_task_handle());
				}
			}

			backoff.snooze();

			// Switch to the next task.
			core_scheduler.reschedule();
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
		if let Some(task) = {
			let mut locked_state = self.state.lock();
			locked_state.count += 1;
			locked_state.queue.pop()
		} {
			// Wake up any task that has been waiting for this semaphore.
			core_scheduler().custom_wakeup(task);
		};
	}
}
