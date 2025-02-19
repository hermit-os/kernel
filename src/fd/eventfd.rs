use alloc::boxed::Box;
use alloc::collections::vec_deque::VecDeque;
use core::future::{self, Future};
use core::mem::{self, MaybeUninit};
use core::task::{Poll, Waker, ready};

use async_lock::Mutex;
use async_trait::async_trait;

use crate::fd::{EventFlags, ObjectInterface, PollEvent};
use crate::io;

#[derive(Debug)]
struct EventState {
	pub counter: u64,
	pub read_queue: VecDeque<Waker>,
	pub write_queue: VecDeque<Waker>,
}

impl EventState {
	pub fn new(counter: u64) -> Self {
		Self {
			counter,
			read_queue: VecDeque::new(),
			write_queue: VecDeque::new(),
		}
	}
}

#[derive(Debug)]
pub(crate) struct EventFd {
	state: Mutex<EventState>,
	flags: EventFlags,
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
	async fn read(&self, buf: &mut [MaybeUninit<u8>]) -> io::Result<usize> {
		let len = mem::size_of::<u64>();

		if buf.len() < len {
			return Err(io::Error::EINVAL);
		}

		future::poll_fn(|cx| {
			if self.flags.contains(EventFlags::EFD_SEMAPHORE) {
				let mut pinned = core::pin::pin!(self.state.lock());
				let mut guard = ready!(pinned.as_mut().poll(cx));
				if guard.counter > 0 {
					guard.counter -= 1;
					buf[..len].write_copy_of_slice(&u64::to_ne_bytes(1));
					if let Some(cx) = guard.write_queue.pop_front() {
						cx.wake_by_ref();
					}
					Poll::Ready(Ok(len))
				} else {
					guard.read_queue.push_back(cx.waker().clone());
					Poll::Pending
				}
			} else {
				let mut pinned = core::pin::pin!(self.state.lock());
				let mut guard = ready!(pinned.as_mut().poll(cx));
				let tmp = guard.counter;
				if tmp > 0 {
					guard.counter = 0;
					buf[..len].write_copy_of_slice(&u64::to_ne_bytes(tmp));
					if let Some(cx) = guard.read_queue.pop_front() {
						cx.wake_by_ref();
					}
					Poll::Ready(Ok(len))
				} else if self.flags.contains(EventFlags::EFD_NONBLOCK) {
					Poll::Ready(Err(io::Error::EAGAIN))
				} else {
					guard.read_queue.push_back(cx.waker().clone());
					Poll::Pending
				}
			}
		})
		.await
	}

	async fn write(&self, buf: &[u8]) -> io::Result<usize> {
		let len = mem::size_of::<u64>();

		if buf.len() < len {
			return Err(io::Error::EINVAL);
		}

		let c = u64::from_ne_bytes(buf[..len].try_into().unwrap());

		future::poll_fn(|cx| {
			let mut pinned = core::pin::pin!(self.state.lock());
			let mut guard = ready!(pinned.as_mut().poll(cx));
			if u64::MAX - guard.counter > c {
				guard.counter += c;
				if self.flags.contains(EventFlags::EFD_SEMAPHORE) {
					for _i in 0..c {
						if let Some(cx) = guard.read_queue.pop_front() {
							cx.wake_by_ref();
						} else {
							break;
						}
					}
				} else if let Some(cx) = guard.read_queue.pop_front() {
					cx.wake_by_ref();
				}

				Poll::Ready(Ok(len))
			} else if self.flags.contains(EventFlags::EFD_NONBLOCK) {
				Poll::Ready(Err(io::Error::EAGAIN))
			} else {
				guard.write_queue.push_back(cx.waker().clone());
				Poll::Pending
			}
		})
		.await
	}

	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		let guard = self.state.lock().await;

		let mut available = PollEvent::empty();

		if guard.counter < u64::MAX - 1 {
			available.insert(PollEvent::POLLOUT | PollEvent::POLLWRNORM | PollEvent::POLLWRBAND);
		}

		if guard.counter > 0 {
			available.insert(PollEvent::POLLIN | PollEvent::POLLRDNORM | PollEvent::POLLRDBAND);
		}

		drop(guard);

		let ret = event & available;

		future::poll_fn(|cx| {
			if ret.is_empty() {
				let mut pinned = core::pin::pin!(self.state.lock());
				let mut guard = ready!(pinned.as_mut().poll(cx));
				if event
					.intersects(PollEvent::POLLIN | PollEvent::POLLRDNORM | PollEvent::POLLRDNORM)
				{
					guard.read_queue.push_back(cx.waker().clone());
					Poll::Pending
				} else if event
					.intersects(PollEvent::POLLOUT | PollEvent::POLLWRNORM | PollEvent::POLLWRBAND)
				{
					guard.write_queue.push_back(cx.waker().clone());
					Poll::Pending
				} else {
					Poll::Ready(Ok(ret))
				}
			} else {
				Poll::Ready(Ok(ret))
			}
		})
		.await
	}
}
