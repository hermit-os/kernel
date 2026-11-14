use alloc::collections::vec_deque::VecDeque;
use alloc::vec::Vec;
use core::future;
use core::task::{Poll, Waker};

use hermit_sync::InterruptTicketMutex;

use crate::errno::Errno;
use crate::fd::{EventFlags, ObjectInterface, PollEvent};
use crate::io;

// The state is protected by an `InterruptTicketMutex` instead of an
// `async_lock::Mutex`.
//
// The critical sections are short and never block, so there is nothing to
// wait for while holding the lock. With an `async_lock::Mutex`, the lock future
// was created anew inside `poll_fn` (`pin!(self.state.lock())`) and dropped at
// the end of every poll. If another core held the lock at that moment, the lock
// future returned `Pending` and its listener was dropped together with it, so
// nobody would ever poll the operation again: a lost wakeup.
//
// This is easy to hit with tokio: a `spawn_blocking` thread finishes and wakes
// the I/O driver through mio's eventfd waker while the driver thread is inside
// `poll(2)` on the same eventfd. The `write` gets lost and the runtime stops
// making progress.
//
// Now `Pending` is only returned for a semantic reason (counter empty or full),
// and the waker is enqueued under the lock, so the wakeup cannot be missed.
// Wakers are invoked after the lock is released.

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

pub(crate) struct EventFd {
	state: InterruptTicketMutex<EventState>,
	flags: EventFlags,
}

impl EventFd {
	pub fn new(initval: u64, flags: EventFlags) -> Self {
		debug!("Create EventFd {initval}, {flags:?}");
		Self {
			state: InterruptTicketMutex::new(EventState::new(initval)),
			flags,
		}
	}
}

/// Enqueues the waker unless an equivalent one is already queued.
///
/// `poll`-based multiplexing registers on every poll, so without this check the
/// queue keeps growing while no event arrives.
fn enqueue(queue: &mut VecDeque<Waker>, waker: &Waker) {
	if !queue.iter().any(|w| w.will_wake(waker)) {
		queue.push_back(waker.clone());
	}
}

/// Wakes the given wakers. Must be called without holding the state lock.
fn wake_all(wakers: Vec<Waker>) {
	for waker in wakers {
		waker.wake();
	}
}

impl ObjectInterface for EventFd {
	async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
		let len = size_of::<u64>();

		if buf.len() < len {
			return Err(Errno::Inval);
		}

		future::poll_fn(|cx| {
			let mut guard = self.state.lock();
			if guard.counter > 0 {
				let value = if self.flags.contains(EventFlags::EFD_SEMAPHORE) {
					guard.counter -= 1;
					1
				} else {
					core::mem::replace(&mut guard.counter, 0)
				};
				buf[..len].copy_from_slice(&u64::to_ne_bytes(value));
				// The counter decreased, so pending writers may be able to proceed.
				let wakers: Vec<Waker> = guard.write_queue.drain(..).collect();
				drop(guard);
				wake_all(wakers);
				Poll::Ready(Ok(len))
			} else if self.flags.contains(EventFlags::EFD_NONBLOCK) {
				Poll::Ready(Err(Errno::Again))
			} else {
				enqueue(&mut guard.read_queue, cx.waker());
				Poll::Pending
			}
		})
		.await
	}

	async fn write(&self, buf: &[u8]) -> io::Result<usize> {
		let len = size_of::<u64>();

		if buf.len() < len {
			return Err(Errno::Inval);
		}

		let c = u64::from_ne_bytes(buf[..len].try_into().unwrap());

		future::poll_fn(|cx| {
			let mut guard = self.state.lock();
			if u64::MAX - guard.counter > c {
				guard.counter += c;
				// The eventfd is now readable. Wake *all* registered pollers, not
				// just one: a readable notification is level-triggered, and the
				// `poll`-based fd multiplexing re-registers a fresh waker on every
				// poll, so the queue may contain stale wakers.
				let wakers: Vec<Waker> = guard.read_queue.drain(..).collect();
				drop(guard);
				wake_all(wakers);
				Poll::Ready(Ok(len))
			} else if self.flags.contains(EventFlags::EFD_NONBLOCK) {
				Poll::Ready(Err(Errno::Again))
			} else {
				enqueue(&mut guard.write_queue, cx.waker());
				Poll::Pending
			}
		})
		.await
	}

	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		future::poll_fn(|cx| {
			let mut guard = self.state.lock();

			// Re-evaluate readiness on every poll.
			let mut available = PollEvent::empty();
			if guard.counter < u64::MAX - 1 {
				available
					.insert(PollEvent::POLLOUT | PollEvent::POLLWRNORM | PollEvent::POLLWRBAND);
			}
			if guard.counter > 0 {
				available.insert(PollEvent::POLLIN | PollEvent::POLLRDNORM | PollEvent::POLLRDBAND);
			}

			let ret = event & available;
			if !ret.is_empty() {
				return Poll::Ready(Ok(ret));
			}

			let wants_in =
				event.intersects(PollEvent::POLLIN | PollEvent::POLLRDNORM | PollEvent::POLLRDBAND);
			let wants_out = event
				.intersects(PollEvent::POLLOUT | PollEvent::POLLWRNORM | PollEvent::POLLWRBAND);
			if wants_in {
				enqueue(&mut guard.read_queue, cx.waker());
			}
			if wants_out {
				enqueue(&mut guard.write_queue, cx.waker());
			}
			if wants_in || wants_out {
				Poll::Pending
			} else {
				Poll::Ready(Ok(ret))
			}
		})
		.await
	}
}
