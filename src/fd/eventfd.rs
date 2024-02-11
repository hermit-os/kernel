use alloc::boxed::Box;
use alloc::collections::vec_deque::VecDeque;
use core::future::{self, Future};
use core::mem;
use core::task::{Poll, Waker};

use async_lock::Mutex;
use async_trait::async_trait;

use crate::fd::{block_on, EventFlags, IoError, ObjectInterface, PollEvent};

#[derive(Debug)]
struct EventState {
	pub counter: u64,
	pub queue: VecDeque<Waker>,
}

impl EventState {
	pub fn new(counter: u64) -> Self {
		Self {
			counter,
			queue: VecDeque::new(),
		}
	}
}

#[derive(Debug)]
pub(crate) struct EventFd {
	state: Mutex<EventState>,
	flags: EventFlags,
}

impl Clone for EventFd {
	fn clone(&self) -> Self {
		let counter = block_on(async { Ok(self.state.lock().await.counter) }, None).unwrap();
		Self {
			state: Mutex::new(EventState::new(counter)),
			flags: self.flags,
		}
	}
}

impl EventFd {
	pub fn new(initval: u64, flags: EventFlags) -> Self {
		debug!("Create EventFd {}, {:?}", initval, flags);
		Self {
			state: Mutex::new(EventState::new(initval)),
			flags,
		}
	}
}

#[async_trait]
impl ObjectInterface for EventFd {
	async fn async_read(&self, buf: &mut [u8]) -> Result<usize, IoError> {
		let len = mem::size_of::<u64>();

		if buf.len() < len {
			return Err(IoError::EINVAL);
		}

		future::poll_fn(|cx| {
			if self.flags.contains(EventFlags::EFD_SEMAPHORE) {
				let mut pinned = core::pin::pin!(self.state.lock());
				if let Poll::Ready(mut guard) = pinned.as_mut().poll(cx) {
					if guard.counter > 0 {
						guard.counter -= 1;
						let tmp = u64::to_ne_bytes(1);
						buf[..len].copy_from_slice(&tmp);
						Poll::Ready(Ok(len))
					} else {
						guard.queue.push_back(cx.waker().clone());
						Poll::Pending
					}
				} else {
					Poll::Pending
				}
			} else {
				let mut pinned = core::pin::pin!(self.state.lock());
				if let Poll::Ready(mut guard) = pinned.as_mut().poll(cx) {
					let tmp = guard.counter;
					if tmp > 0 {
						guard.counter = 0;
						buf[..len].copy_from_slice(&u64::to_ne_bytes(tmp));
						Poll::Ready(Ok(len))
					} else {
						guard.queue.push_back(cx.waker().clone());
						Poll::Pending
					}
				} else {
					Poll::Pending
				}
			}
		})
		.await
	}

	async fn async_write(&self, buf: &[u8]) -> Result<usize, IoError> {
		let len = mem::size_of::<u64>();

		if buf.len() < len {
			return Err(IoError::EINVAL);
		}

		let c = u64::from_ne_bytes(buf[..len].try_into().unwrap());
		if self.flags.contains(EventFlags::EFD_SEMAPHORE) {
			let mut guard = self.state.lock().await;
			for _i in 0..c {
				if guard.counter == u64::MAX - 1 {
					if self.is_nonblocking() {
						return Err(IoError::EAGAIN);
					} else {
						// TODO: task should be blocked until the addition is possible
						return Err(IoError::EINVAL);
					}
				}
				guard.counter += 1;
				if let Some(cx) = guard.queue.pop_front() {
					cx.wake_by_ref();
				}
			}
		} else {
			let mut guard = self.state.lock().await;
			if guard.counter == u64::MAX - c - 1 {
				if self.is_nonblocking() {
					return Err(IoError::EAGAIN);
				} else {
					// TODO: task should be blocked until the addition is possible
					return Err(IoError::EINVAL);
				}
			}
			guard.counter += c;
			if let Some(cx) = guard.queue.pop_front() {
				cx.wake_by_ref();
			}
		}

		Ok(len)
	}

	async fn poll(&self, event: PollEvent) -> Result<PollEvent, IoError> {
		let mut result: PollEvent = PollEvent::empty();

		if event.contains(PollEvent::POLLOUT) {
			result.insert(PollEvent::POLLOUT);
		}
		if event.contains(PollEvent::POLLWRNORM) {
			result.insert(PollEvent::POLLWRNORM);
		}
		if event.contains(PollEvent::POLLWRBAND) {
			result.insert(PollEvent::POLLWRBAND);
		}

		if self.state.lock().await.counter > 0 {
			if event.contains(PollEvent::POLLIN) {
				result.insert(PollEvent::POLLIN);
			}
			if event.contains(PollEvent::POLLRDNORM) {
				result.insert(PollEvent::POLLRDNORM);
			}
			if event.contains(PollEvent::POLLRDBAND) {
				result.insert(PollEvent::POLLRDBAND);
			}
		}

		future::poll_fn(|cx| {
			if result.is_empty() {
				let mut pinned = core::pin::pin!(self.state.lock());
				if let Poll::Ready(mut guard) = pinned.as_mut().poll(cx) {
					guard.queue.push_back(cx.waker().clone());
				}
				Poll::Pending
			} else {
				Poll::Ready(Ok(result))
			}
		})
		.await
	}

	fn is_nonblocking(&self) -> bool {
		self.flags.contains(EventFlags::EFD_NONBLOCK)
	}
}
