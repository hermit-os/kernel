#![allow(dead_code)]

use alloc::boxed::Box;
use core::fmt;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicU32, Ordering};
use core::task::{Context, Poll};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct AsyncTaskId(u32);

impl fmt::Display for AsyncTaskId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}", self.0)
	}
}

impl AsyncTaskId {
	fn new() -> Self {
		static NEXT_ID: AtomicU32 = AtomicU32::new(0);
		AsyncTaskId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
	}
}

pub(crate) struct AsyncTask {
	id: AsyncTaskId,
	future: Pin<Box<dyn Future<Output = ()>>>,
}

impl AsyncTask {
	pub fn new(future: impl Future<Output = ()> + 'static) -> AsyncTask {
		AsyncTask {
			id: AsyncTaskId::new(),
			future: Box::pin(future),
		}
	}
}

impl Future for AsyncTask {
	type Output = ();

	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		trace!("Run async task {}", self.id);
		self.as_mut().future.as_mut().poll(cx)
	}
}
