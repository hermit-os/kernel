#![allow(dead_code)]

#[cfg(any(feature = "tcp", feature = "udp"))]
pub(crate) mod device;
#[cfg(any(feature = "tcp", feature = "udp"))]
pub(crate) mod network;
pub(crate) mod task;

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
#[cfg(any(feature = "tcp", feature = "udp"))]
use crate::executor::network::network_delay;
use crate::executor::task::AsyncTask;
use crate::fd::IoError;
#[cfg(any(feature = "tcp", feature = "udp"))]
use crate::scheduler::PerCoreSchedulerExt;
use crate::synch::futex::*;

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
		// is not zero, someone already wanted to wakeup a taks and stored another
		// value to the futex address. In this case, the function directly returns
		// and doesn't block.
		let _ = futex_wait_and_set(&self.futex, 0, timeout, Flags::RELATIVE, 0);
	}
}

impl Wake for TaskNotify {
	fn wake(self: Arc<Self>) {
		self.wake_by_ref()
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
		})
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
}

#[inline]
pub(crate) fn now() -> u64 {
	crate::arch::kernel::systemtime::now_micros()
}

/// Blocks the current thread on `f`, running the executor when idling.
pub(crate) fn poll_on<F, T>(future: F, timeout: Option<Duration>) -> Result<T, IoError>
where
	F: Future<Output = Result<T, IoError>>,
{
	#[cfg(any(feature = "tcp", feature = "udp"))]
	let nic = get_network_driver();

	// disable network interrupts
	#[cfg(any(feature = "tcp", feature = "udp"))]
	let no_retransmission = if let Some(nic) = nic {
		let mut guard = nic.lock();
		guard.set_polling_mode(true);
		guard.get_checksums().tcp.tx()
	} else {
		true
	};

	let start = now();
	let mut cx = Context::from_waker(Waker::noop());
	let mut future = pin!(future);

	loop {
		// run background tasks
		run();

		if let Poll::Ready(t) = future.as_mut().poll(&mut cx) {
			#[cfg(any(feature = "tcp", feature = "udp"))]
			if !no_retransmission {
				let wakeup_time =
					network_delay(Instant::from_micros_const(now().try_into().unwrap()))
						.map(|d| crate::arch::processor::get_timer_ticks() + d.total_micros());
				core_scheduler().add_network_timer(wakeup_time);
			}

			// allow network interrupts
			#[cfg(any(feature = "tcp", feature = "udp"))]
			if let Some(nic) = nic {
				nic.lock().set_polling_mode(false);
			}

			return t;
		}

		if let Some(duration) = timeout {
			if Duration::from_micros(now() - start) >= duration {
				#[cfg(any(feature = "tcp", feature = "udp"))]
				if !no_retransmission {
					let wakeup_time =
						network_delay(Instant::from_micros_const(now().try_into().unwrap()))
							.map(|d| crate::arch::processor::get_timer_ticks() + d.total_micros());
					core_scheduler().add_network_timer(wakeup_time);
				}

				// allow network interrupts
				#[cfg(any(feature = "tcp", feature = "udp"))]
				if let Some(nic) = nic {
					nic.lock().set_polling_mode(false);
				}

				return Err(IoError::ETIME);
			}
		}
	}
}

/// Blocks the current thread on `f`, running the executor when idling.
pub(crate) fn block_on<F, T>(future: F, timeout: Option<Duration>) -> Result<T, IoError>
where
	F: Future<Output = Result<T, IoError>>,
{
	#[cfg(any(feature = "tcp", feature = "udp"))]
	let nic = get_network_driver();

	// disable network interrupts
	#[cfg(any(feature = "tcp", feature = "udp"))]
	let no_retransmission = if let Some(nic) = nic {
		let mut guard = nic.lock();
		guard.set_polling_mode(true);
		!guard.get_checksums().tcp.tx()
	} else {
		true
	};

	let backoff = Backoff::new();
	let start = now();
	let task_notify = Arc::new(TaskNotify::new());
	let waker = task_notify.clone().into();
	let mut cx = Context::from_waker(&waker);
	let mut future = pin!(future);

	loop {
		// run background tasks
		run();

		let now = now();
		if let Poll::Ready(t) = future.as_mut().poll(&mut cx) {
			#[cfg(any(feature = "tcp", feature = "udp"))]
			if !no_retransmission {
				let network_timer =
					network_delay(Instant::from_micros_const(now.try_into().unwrap()))
						.map(|d| crate::arch::processor::get_timer_ticks() + d.total_micros());
				core_scheduler().add_network_timer(network_timer);
			}

			// allow network interrupts
			#[cfg(any(feature = "tcp", feature = "udp"))]
			if let Some(nic) = nic {
				nic.lock().set_polling_mode(false);
			}

			return t;
		}

		if let Some(duration) = timeout {
			if Duration::from_micros(now - start) >= duration {
				#[cfg(any(feature = "tcp", feature = "udp"))]
				if !no_retransmission {
					let network_timer =
						network_delay(Instant::from_micros_const(now.try_into().unwrap()))
							.map(|d| crate::arch::processor::get_timer_ticks() + d.total_micros());
					core_scheduler().add_network_timer(network_timer);
				}

				// allow network interrupts
				#[cfg(any(feature = "tcp", feature = "udp"))]
				if let Some(nic) = nic {
					nic.lock().set_polling_mode(false);
				}

				return Err(IoError::ETIME);
			}
		}

		#[cfg(any(feature = "tcp", feature = "udp"))]
		{
			let delay = network_delay(Instant::from_micros_const(now.try_into().unwrap()))
				.map(|d| d.total_micros());

			if backoff.is_completed() && delay.unwrap_or(10_000_000) > 10_000 {
				let wakeup_time =
					timeout.map(|duration| start + u64::try_from(duration.as_micros()).unwrap());
				if !no_retransmission {
					let ticks = crate::arch::processor::get_timer_ticks();
					let network_timer = delay.map(|d| ticks + d);
					core_scheduler().add_network_timer(network_timer);
				}

				// allow network interrupts
				if let Some(nic) = nic {
					nic.lock().set_polling_mode(false);
				}

				// switch to another task
				task_notify.wait(wakeup_time);

				// restore default values
				if let Some(nic) = nic {
					nic.lock().set_polling_mode(true);
				}
				backoff.reset();
			} else {
				backoff.snooze();
			}
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
