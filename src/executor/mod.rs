#![allow(dead_code)]

#[cfg(feature = "tcp")]
pub(crate) mod device;
#[cfg(feature = "tcp")]
pub(crate) mod network;
pub(crate) mod task;

use alloc::sync::Arc;
use alloc::task::Wake;
use core::future::Future;
use core::task::{Context, Poll, Waker};

use hermit_sync::without_interrupts;

use crate::arch::core_local::*;
use crate::executor::task::AsyncTask;
use crate::scheduler::task::TaskHandle;

struct TaskNotify {
	/// The single task executor .
	handle: TaskHandle,
}

impl TaskNotify {
	pub fn new() -> Self {
		Self {
			handle: core_scheduler().get_current_task_handle(),
		}
	}
}

impl Wake for TaskNotify {
	fn wake(self: Arc<Self>) {
		self.wake_by_ref()
	}

	fn wake_by_ref(self: &Arc<Self>) {
		trace!("Wakeup task {}", self.handle.get_id());
		core_scheduler().custom_wakeup(self.handle);
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
