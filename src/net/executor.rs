use alloc::sync::Arc;
use alloc::task::Wake;
use alloc::vec::Vec;
use async_task::{Runnable, Task};
use core::sync::atomic::Ordering;
use core::{
	future::Future,
	sync::atomic::AtomicBool,
	task::{Context, Poll},
};
use futures_lite::pin;
use smoltcp::time::{Duration, Instant};

use crate::core_scheduler;
use crate::scheduler::task::TaskHandle;
use crate::synch::spinlock::{Spinlock, SpinlockIrqSave};

static QUEUE: Spinlock<Vec<Runnable>> = Spinlock::new(Vec::new());

#[inline]
fn network_delay(timestamp: Instant) -> Option<Duration> {
	crate::net::NIC
		.lock()
		.as_nic_mut()
		.unwrap()
		.poll_delay(timestamp)
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
pub(crate) fn block_on<F, T>(future: F, timeout: Option<Duration>) -> Result<T, ()>
where
	F: Future<Output = T>,
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
			return Ok(t);
		}

		if let Some(duration) = timeout {
			if crate::net::now() >= start + duration {
				if let Some(delay) = network_delay(crate::net::now()).map(|d| d.total_micros()) {
					let wakeup_time = crate::arch::processor::get_timer_ticks() + delay;
					core_scheduler().add_network_timer(wakeup_time);
				}

				// allow interrupts => NIC thread is able to run
				set_polling_mode(false);
				return Err(());
			}
		}

		let now = crate::net::now();
		let delay = network_delay(now).map(|d| d.total_micros());
		if delay.unwrap_or(10_000_000) > 100_000 {
			let unparked = task_notify.unparked.swap(false, Ordering::AcqRel);
			if !unparked {
				let core_scheduler = core_scheduler();
				let wakeup_time = delay.map(|us| crate::arch::processor::get_timer_ticks() + us);
				core_scheduler.block_current_task(wakeup_time);
				// allow interrupts => NIC thread is able to run
				set_polling_mode(false);
				// switch to another task
				core_scheduler.reschedule();
				// Polling mode => no NIC interrupts => NIC thread should not run
				set_polling_mode(true);
			}
		}
	}
}

/// set driver in polling mode and threads will not be blocked
fn set_polling_mode(value: bool) {
	static IN_POLLING_MODE: SpinlockIrqSave<usize> = SpinlockIrqSave::new(0);

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
