use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::task::Wake;
use alloc::vec::Vec;
use core::future::Future;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::{Context, Poll};

use async_task::{Runnable, Task};
use futures_lite::pin;
use hermit_sync::{InterruptTicketMutex, TicketMutex};
use smoltcp::time::{Duration, Instant};

use crate::core_scheduler;
use crate::scheduler::task::{TaskHandle, TaskId};

static QUEUE: TicketMutex<Vec<Runnable>> = TicketMutex::new(Vec::new());
static BLOCKED_ASYNC_TASKS: TicketMutex<BTreeMap<TaskId, TaskHandle>> =
	TicketMutex::new(BTreeMap::new());

#[inline]
fn network_delay(timestamp: Instant) -> Option<Duration> {
	crate::net::NIC
		.lock()
		.as_nic_mut()
		.unwrap()
		.poll_delay(timestamp)
}

#[inline]
pub(crate) fn wakeup_async_tasks() {
	let scheduler = core_scheduler();
	let mut guard = BLOCKED_ASYNC_TASKS.lock();

	while let Some((_id, handle)) = guard.pop_first() {
		scheduler.custom_wakeup(handle);
	}
}

fn run_executor_once() {
	let mut guard = QUEUE.lock();
	let mut runnables = Vec::with_capacity(guard.len());

	while let Some(runnable) = guard.pop() {
		runnables.push(runnable);
	}

	drop(guard);

	for runnable in runnables {
		runnable.run();
	}
}

/// Spawns a future on the executor.
pub(crate) fn spawn<F, T>(future: F) -> Task<T>
where
	F: Future<Output = T> + Send + 'static,
	T: Send + 'static,
{
	let schedule = |runnable| QUEUE.lock().push(runnable);
	let (runnable, task) = async_task::spawn(future, schedule);
	runnable.schedule();
	task
}

struct TaskNotify {
	/// The single task executor .
	handle: TaskHandle,
	/// A flag to ensure a wakeup is not "forgotten" before the next `block_current_task`
	unparked: AtomicBool,
}

impl TaskNotify {
	pub fn new() -> Self {
		Self {
			handle: core_scheduler().get_current_task_handle(),
			unparked: AtomicBool::new(false),
		}
	}
}

impl Drop for TaskNotify {
	fn drop(&mut self) {
		debug!("Dropping ThreadNotify!");
	}
}

impl Wake for TaskNotify {
	fn wake(self: Arc<Self>) {
		self.wake_by_ref()
	}

	fn wake_by_ref(self: &Arc<Self>) {
		// Make sure the wakeup is remembered until the next `park()`.
		let unparked = self.unparked.swap(true, Ordering::AcqRel);
		if !unparked {
			core_scheduler().custom_wakeup(self.handle);
		}
	}
}

/// Blocks the current thread on `f`, running the executor when idling.
pub(crate) fn block_on<F, T>(future: F, timeout: Option<Duration>) -> Result<T, i32>
where
	F: Future<Output = Result<T, i32>>,
{
	// Polling mode => no NIC interrupts => NIC thread should not run
	set_polling_mode(true);

	let start = crate::net::now();
	let task_notify = Arc::new(TaskNotify::new());
	let waker = task_notify.clone().into();
	let mut cx = Context::from_waker(&waker);
	pin!(future);

	loop {
		// run background tasks
		run_executor_once();

		if let Poll::Ready(t) = future.as_mut().poll(&mut cx) {
			if let Some(delay) = network_delay(crate::net::now()).map(|d| d.total_micros()) {
				let wakeup_time = crate::arch::processor::get_timer_ticks() + delay;
				core_scheduler().add_network_timer(wakeup_time);
			}

			// allow interrupts => NIC thread is able to run
			set_polling_mode(false);
			return t;
		}

		if let Some(duration) = timeout {
			if crate::net::now() >= start + duration {
				if let Some(delay) = network_delay(crate::net::now()).map(|d| d.total_micros()) {
					let wakeup_time = crate::arch::processor::get_timer_ticks() + delay;
					core_scheduler().add_network_timer(wakeup_time);
				}

				// allow interrupts => NIC thread is able to run
				set_polling_mode(false);
				return Err(-crate::errno::ETIME);
			}
		}

		let now = crate::net::now();
		let delay = network_delay(now).map(|d| d.total_micros());
		if delay.unwrap_or(10_000_000) > 100_000 {
			let unparked = task_notify.unparked.swap(false, Ordering::AcqRel);
			if !unparked {
				let core_scheduler = core_scheduler();
				let task = core_scheduler.get_current_task_handle();
				let wakeup_time = delay.map(|us| crate::arch::processor::get_timer_ticks() + us);
				BLOCKED_ASYNC_TASKS
					.lock()
					.insert(task.get_id(), task.clone());
				core_scheduler.block_current_task(wakeup_time);
				// allow interrupts => NIC thread is able to run
				set_polling_mode(false);
				// switch to another task
				core_scheduler.reschedule();
				BLOCKED_ASYNC_TASKS.lock().remove(&task.get_id());
				// Polling mode => no NIC interrupts => NIC thread should not run
				set_polling_mode(true);
			}
		}
	}
}

/// set driver in polling mode and threads will not be blocked
fn set_polling_mode(value: bool) {
	static IN_POLLING_MODE: InterruptTicketMutex<usize> = InterruptTicketMutex::new(0);

	let mut guard = IN_POLLING_MODE.lock();

	if value {
		*guard += 1;

		if *guard == 1 {
			#[cfg(feature = "pci")]
			if let Some(driver) = crate::arch::kernel::pci::get_network_driver() {
				driver.lock().set_polling_mode(value)
			}
		}
	} else {
		*guard -= 1;

		if *guard == 0 {
			#[cfg(feature = "pci")]
			if let Some(driver) = crate::arch::kernel::pci::get_network_driver() {
				driver.lock().set_polling_mode(value)
			}
		}
	}
}
