// Copyright (c) 2019 Stefan Lankes, RWTH Aachen University
// 				 2020 Frederik Schulz, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

#[cfg(feature = "pci")]
pub mod virtio_net;

use crate::arch::kernel::percore::*;
use crate::scheduler::task::TaskHandle;
use crate::synch::semaphore::*;
use crate::synch::spinlock::SpinlockIrqSave;
use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicBool, Ordering};

static NET_SEM: Semaphore = Semaphore::new(0);
static NIC_QUEUE: SpinlockIrqSave<BTreeMap<usize, TaskHandle>> =
	SpinlockIrqSave::new(BTreeMap::new());
static POLLING: AtomicBool = AtomicBool::new(false);

/// period (in usec) to check, if the driver should still use the polling mode
const POLL_PERIOD: u64 = 20_000;

/// set driver in polling mode and threads will not be blocked
fn set_polling_mode(value: bool) {
	// is the driver already in polling mode?
	if POLLING.swap(value, Ordering::SeqCst) != value {
		#[cfg(feature = "pci")]
		if let Some(driver) = crate::arch::kernel::pci::get_network_driver() {
			driver.lock().set_polling_mode(value)
		}

		// wakeup network thread to sleep for longer time
		NET_SEM.release();
	}
}

pub fn netwakeup() {
	NET_SEM.release();
}

pub fn netwait_and_wakeup(handles: &[usize], millis: Option<u64>) {
	// do we have to wakeup a thread?
	if handles.len() > 0 {
		let mut guard = NIC_QUEUE.lock();

		for i in handles {
			if let Some(task) = guard.remove(i) {
				core_scheduler().custom_wakeup(task);
			}
		}
	}

	let mut reset_nic = false;

	// check if the driver should be in the polling mode
	while POLLING.swap(false, Ordering::SeqCst) == true {
		reset_nic = true;

		let core_scheduler = core_scheduler();
		let wakeup_time = Some(crate::arch::processor::get_timer_ticks() + POLL_PERIOD);

		core_scheduler.block_current_task(wakeup_time);

		// Switch to the next task.
		core_scheduler.reschedule();
	}

	if reset_nic {
		#[cfg(feature = "pci")]
		if let Some(driver) = crate::arch::kernel::pci::get_network_driver() {
			driver.lock().set_polling_mode(false);
		};
	} else {
		NET_SEM.acquire(millis);
	}
}

pub fn netwait(handle: usize, millis: Option<u64>) {
	// smoltcp want to poll the nic
	let is_polling = if let Some(t) = millis { t == 0 } else { false };

	if is_polling {
		set_polling_mode(true);
	} else {
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
