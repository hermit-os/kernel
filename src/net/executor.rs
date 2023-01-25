use alloc::sync::Arc;
use alloc::task::Wake;
use alloc::vec::Vec;
use core::future::Future;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::{Context, Poll};

use async_task::{Runnable, Task};
use futures_lite::pin;
use hermit_sync::InterruptTicketMutex;
use smoltcp::time::{Duration, Instant};

use crate::core_scheduler;
use crate::scheduler::task::TaskHandle;

static QUEUE: InterruptTicketMutex<Vec<Runnable>> = InterruptTicketMutex::new(Vec::new());

/// set driver in polling mode
#[inline]
fn set_polling_mode(value: bool) {
	#[cfg(feature = "pci")]
	if let Some(driver) = crate::arch::kernel::pci::get_network_driver() {
		driver.lock().set_polling_mode(value)
	}
}

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
pub(crate) fn block_on<F, T>(future: F, timeout: Option<Duration>) -> Result<T, i32>
where
	F: Future<Output = Result<T, i32>>,
{
	// Enter polling mode => no NIC interrupts
	set_polling_mode(true);

	let mut counter: u16 = 0;
	let start = crate::net::now();
	let task_notify = Arc::new(TaskNotify::new());
	let waker = task_notify.clone().into();
	let mut cx = Context::from_waker(&waker);
	pin!(future);

	loop {
		// run background tasks
		run_executor_once();

		if let Poll::Ready(t) = future.as_mut().poll(&mut cx) {
			let wakeup_time = network_delay(crate::net::now())
				.map(|d| crate::arch::processor::get_timer_ticks() + d.total_micros());
			core_scheduler().add_network_timer(wakeup_time);

			// allow interrupts => NIC thread is able to run
			set_polling_mode(false);

			return t;
		}

		if let Some(duration) = timeout {
			if crate::net::now() >= start + duration {
				let wakeup_time = network_delay(crate::net::now())
					.map(|d| crate::arch::processor::get_timer_ticks() + d.total_micros());
				core_scheduler().add_network_timer(wakeup_time);

				// allow interrupts => NIC thread is able to run
				set_polling_mode(false);

				return Err(-crate::errno::ETIME);
			}
		}

		counter += 1;
		let now = crate::net::now();
		let delay = network_delay(now).map(|d| d.total_micros());
		if counter > 200 && delay.unwrap_or(10_000_000) > 100_000 {
			let unparked = task_notify.unparked.swap(false, Ordering::AcqRel);
			if !unparked {
				let core_scheduler = core_scheduler();
				core_scheduler.add_network_timer(
					delay.map(|d| crate::arch::processor::get_timer_ticks() + d),
				);
				let wakeup_time = delay.map(|d| crate::arch::processor::get_timer_ticks() + d);
				core_scheduler.add_network_timer(wakeup_time);
				core_scheduler.block_current_async_task();
				// allow interrupts => NIC thread is able to run
				set_polling_mode(false);
				// switch to another task
				core_scheduler.reschedule();
				// Polling mode => no NIC interrupts => NIC thread should not run
				set_polling_mode(true);
				counter = 0;
			}
		}
	}
}
