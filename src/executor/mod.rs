#![allow(dead_code)]

#[cfg(feature = "allocation-stats")]
mod alloc_stats;
#[cfg(feature = "balloon")]
mod balloon;
#[cfg(any(feature = "tcp", feature = "udp"))]
pub(crate) mod device;
#[cfg(any(feature = "tcp", feature = "udp"))]
pub(crate) mod network;
pub(crate) mod task;
#[cfg(feature = "vsock")]
pub(crate) mod vsock;

use alloc::sync::Arc;
use alloc::task::Wake;
use core::future::Future;
use core::pin::pin;
use core::sync::atomic::AtomicU32;
use core::task::{Context, Poll, Waker};
use core::time::Duration;

use crossbeam_utils::Backoff;
use hermit_sync::without_interrupts;
#[cfg(any(feature = "tcp", feature = "udp"))]
use smoltcp::time::Instant;

use crate::arch::core_local::*;
#[cfg(all(any(feature = "tcp", feature = "udp"), not(feature = "pci")))]
use crate::drivers::mmio::get_network_driver;
#[cfg(any(feature = "tcp", feature = "udp"))]
use crate::drivers::net::NetworkDriver;
#[cfg(all(any(feature = "tcp", feature = "udp"), feature = "pci"))]
use crate::drivers::pci::get_network_driver;
use crate::executor::task::AsyncTask;
use crate::io;
#[cfg(any(feature = "tcp", feature = "udp"))]
use crate::scheduler::PerCoreSchedulerExt;
use crate::synch::futex::*;

/// WakerRegistration is derived from smoltcp's
/// implementation.
#[derive(Debug)]
pub(crate) struct WakerRegistration {
	waker: Option<Waker>,
}

impl WakerRegistration {
	pub const fn new() -> Self {
		Self { waker: None }
	}

	/// Register a waker. Overwrites the previous waker, if any.
	pub fn register(&mut self, w: &Waker) {
		match self.waker {
			// Optimization: If both the old and new Wakers wake the same task, we can simply
			// keep the old waker, skipping the clone.
			Some(ref w2) if (w2.will_wake(w)) => {}
			// In all other cases
			// - we have no waker registered
			// - we have a waker registered but it's for a different task.
			// then clone the new waker and store it
			_ => self.waker = Some(w.clone()),
		}
	}

	/// Wake the registered waker, if any.
	pub fn wake(&mut self) {
		if let Some(w) = self.waker.take() {
			w.wake();
		}
	}
}

struct TaskNotify {
	/// Futex to wakeup a single task
	futex: AtomicU32,
}

impl TaskNotify {
	pub const fn new() -> Self {
		Self {
			futex: AtomicU32::new(0),
		}
	}

	pub fn wait(&self, timeout: Option<u64>) {
		// Wait for a futex and reset the value to zero. If the value
		// is not zero, someone already wanted to wakeup a task and stored another
		// value to the futex address. In this case, the function directly returns
		// and doesn't block.
		let _ = futex_wait_and_set(&self.futex, 0, timeout, Flags::RELATIVE, 0);
	}
}

impl Wake for TaskNotify {
	fn wake(self: Arc<Self>) {
		self.wake_by_ref();
	}

	fn wake_by_ref(self: &Arc<Self>) {
		let _ = futex_wake_or_set(&self.futex, 1, u32::MAX);
	}
}

pub(crate) fn run() {
	let mut cx = Context::from_waker(Waker::noop());

	without_interrupts(|| {
		async_tasks().retain_mut(|task| {
			trace!("Run async task {}", task.id());

			match task.poll(&mut cx) {
				Poll::Ready(()) => false,
				Poll::Pending => true,
			}
		});
	});
}

/// Spawns a future on the executor.
pub(crate) fn spawn<F>(future: F)
where
	F: Future<Output = ()> + Send + 'static,
{
	without_interrupts(|| async_tasks().push(AsyncTask::new(future)));
}

pub fn init() {
	#[cfg(any(feature = "tcp", feature = "udp"))]
	crate::executor::network::init();
	#[cfg(feature = "vsock")]
	crate::executor::vsock::init();
	#[cfg(feature = "allocation-stats")]
	crate::executor::alloc_stats::init();
	#[cfg(feature = "balloon")]
	crate::executor::balloon::init();
}

/// Blocks the current thread on `f`, running the executor when idling.
pub(crate) fn poll_on<F, T>(future: F) -> io::Result<T>
where
	F: Future<Output = io::Result<T>>,
{
	let mut cx = Context::from_waker(Waker::noop());
	let mut future = pin!(future);

	loop {
		// run background tasks
		run();

		if let Poll::Ready(t) = future.as_mut().poll(&mut cx) {
			return t;
		}
	}
}

/// Blocks the current thread on `f`, running the executor when idling.
pub(crate) fn block_on<F, T>(future: F, timeout: Option<Duration>) -> io::Result<T>
where
	F: Future<Output = io::Result<T>>,
{
	#[cfg(any(feature = "tcp", feature = "udp"))]
	let device = get_network_driver();

	let backoff = Backoff::new();
	let start = crate::arch::kernel::systemtime::now_micros();
	let task_notify = Arc::new(TaskNotify::new());
	let waker = task_notify.clone().into();
	let mut cx = Context::from_waker(&waker);
	let mut future = pin!(future);

	loop {
		// check future
		let result = future.as_mut().poll(&mut cx);

		// run background all tasks, which poll also the network device
		run();

		let now = crate::arch::kernel::systemtime::now_micros();
		if let Poll::Ready(t) = result {
			// allow network interrupts
			#[cfg(any(feature = "tcp", feature = "udp"))]
			{
				let delay = if let Ok(nic) = crate::executor::network::NIC.lock().as_nic_mut() {
					nic.poll_delay(Instant::from_micros_const(now.try_into().unwrap()))
						.map(|d| d.total_micros())
				} else {
					None
				};
				core_scheduler().add_network_timer(
					delay.map(|d| crate::arch::processor::get_timer_ticks() + d),
				);

				if let Some(device) = device {
					device.lock().set_polling_mode(false);
				}
			}

			return t;
		}

		if let Some(duration) = timeout {
			if Duration::from_micros(now - start) >= duration {
				// allow network interrupts
				#[cfg(any(feature = "tcp", feature = "udp"))]
				{
					let delay = if let Ok(nic) = crate::executor::network::NIC.lock().as_nic_mut() {
						nic.poll_delay(Instant::from_micros_const(now.try_into().unwrap()))
							.map(|d| d.total_micros())
					} else {
						None
					};
					core_scheduler().add_network_timer(
						delay.map(|d| crate::arch::processor::get_timer_ticks() + d),
					);

					if let Some(device) = device {
						device.lock().set_polling_mode(false);
					}
				}

				return Err(io::Error::ETIME);
			}
		}

		#[cfg(any(feature = "tcp", feature = "udp"))]
		if backoff.is_completed() {
			let delay = if let Ok(nic) = crate::executor::network::NIC.lock().as_nic_mut() {
				nic.poll_delay(Instant::from_micros_const(now.try_into().unwrap()))
					.map(|d| d.total_micros())
			} else {
				None
			};

			if delay.unwrap_or(10_000_000) > 10_000 {
				core_scheduler().add_network_timer(
					delay.map(|d| crate::arch::processor::get_timer_ticks() + d),
				);
				let wakeup_time =
					timeout.map(|duration| start + u64::try_from(duration.as_micros()).unwrap());

				// allow network interrupts
				if let Some(device) = device {
					device.lock().set_polling_mode(false);
				}

				// switch to another task
				task_notify.wait(wakeup_time);

				// restore default values
				if let Some(device) = device {
					device.lock().set_polling_mode(true);
				}

				backoff.reset();
			}
		} else {
			backoff.snooze();
		}

		#[cfg(not(any(feature = "tcp", feature = "udp")))]
		{
			if backoff.is_completed() {
				let wakeup_time =
					timeout.map(|duration| start + u64::try_from(duration.as_micros()).unwrap());

				// switch to another task
				task_notify.wait(wakeup_time);

				// restore default values
				backoff.reset();
			} else {
				backoff.snooze();
			}
		}
	}
}
