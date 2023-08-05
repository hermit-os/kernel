#![allow(dead_code)]

#[cfg(feature = "tcp")]
pub(crate) mod device;
#[cfg(feature = "tcp")]
pub(crate) mod network;
pub(crate) mod task;

use alloc::sync::Arc;
use alloc::task::Wake;
use core::future::Future;
use core::sync::atomic::{AtomicU32, Ordering};
use core::task::{Context, Poll, Waker};

use hermit_sync::without_interrupts;

use crate::arch::core_local::*;
use crate::executor::task::AsyncTask;
use crate::synch::futex::*;

struct TaskNotify {
	/// The single task executor .
	futex: AtomicU32,
}

impl TaskNotify {
	pub const fn new() -> Self {
		Self {
			futex: AtomicU32::new(0),
		}
	}

	pub fn wait(&self, timeout: Option<u64>) {
		let _ = futex_wait_and_set(&self.futex, 0, timeout, Flags::RELATIVE, 0);
	}
}

impl Wake for TaskNotify {
	fn wake(self: Arc<Self>) {
		self.wake_by_ref()
	}

	fn wake_by_ref(self: &Arc<Self>) {
		self.futex.store(u32::MAX, Ordering::SeqCst);
		let _ = futex_wake(&self.futex, 1);
	}
}

pub(crate) fn run() {
	let waker = Waker::noop();
	let mut cx = Context::from_waker(&waker);

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
	#[cfg(all(feature = "tcp", not(feature = "newlib")))]
	crate::executor::network::init();
}
