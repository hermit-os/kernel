use alloc::boxed::Box;
use core::fmt;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicU32, Ordering};
use core::task::{Context, Poll};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct AsyncTaskId(u32);

impl AsyncTaskId {
	pub const fn into(self) -> u32 {
		self.0
	}

	pub const fn from(x: u32) -> Self {
		AsyncTaskId(x)
	}
}

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

	pub fn id(&self) -> AsyncTaskId {
		self.id
	}

	pub fn poll(&mut self, context: &mut Context<'_>) -> Poll<()> {
		self.future.as_mut().poll(context)
	}
}
