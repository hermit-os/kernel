// Copyright (c) 2019 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

#[cfg(feature = "newlib")]
pub use self::newlib::{netwait, netwakeup};
#[cfg(not(feature = "newlib"))]
pub use self::nonewlib::{netwait, netwait_and_wakeup, netwakeup};

#[cfg(feature = "newlib")]
mod newlib {
	use crate::synch::semaphore::*;

	static NET_SEM: Semaphore = Semaphore::new(0);

	pub fn netwakeup() {
		NET_SEM.release();
	}

	pub fn netwait(millis: Option<u64>) {
		match millis {
			Some(ms) => {
				if ms > 0 {
					NET_SEM.acquire(Some(ms));
				} else {
					NET_SEM.try_acquire();
				}
			}
			_ => {
				NET_SEM.acquire(None);
			}
		};
	}
}

#[cfg(not(feature = "newlib"))]
mod nonewlib {
	use crate::arch::kernel::percore::*;
	use crate::scheduler::task::TaskHandle;
	use crate::synch::semaphore::*;
	use crate::synch::spinlock::SpinlockIrqSave;
	use alloc::collections::BTreeMap;

	static NET_SEM: Semaphore = Semaphore::new(0);
	static NIC_QUEUE: SpinlockIrqSave<BTreeMap<usize, TaskHandle>> =
		SpinlockIrqSave::new(BTreeMap::new());

	pub fn netwakeup() {
		NET_SEM.release();
	}

	pub fn netwait_and_wakeup(handles: &[usize], millis: Option<u64>) {
		{
			let mut guard = NIC_QUEUE.lock();

			for i in handles {
				if let Some(task) = guard.remove(i) {
					core_scheduler().custom_wakeup(task);
				}
			}
		}

		NET_SEM.acquire(millis);
	}

	pub fn netwait(handle: usize, millis: Option<u64>) {
		let wakeup_time = match millis {
			Some(ms) => Some(crate::arch::processor::get_timer_ticks() + ms * 1000),
			_ => None,
		};
		let mut guard = NIC_QUEUE.lock();
		let core_scheduler = core_scheduler();

		// Block the current task and add it to the wakeup queue.
		core_scheduler.block_current_task(wakeup_time);
		guard.insert(handle, core_scheduler.get_current_task_handle());

		// release lock
		drop(guard);

		// Switch to the next task.
		core_scheduler.reschedule();

		// if the timer is expired, we have still the task in the btreemap
		// => remove it from the btreemap
		if millis.is_some() {
			let mut guard = NIC_QUEUE.lock();

			guard.remove(&handle);
		}
	}
}
