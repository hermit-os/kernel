//! An in-kernel, unidirectional byte pipe.
//!
//! A pipe couples a read end ([`PipeReceiver`]) and a write end
//! ([`PipeSender`]) through a single shared, bounded ring buffer. Both
//! endpoints are ordinary [`Fd`](crate::fd::Fd) objects, so they live
//! behind `Arc<RwLock<Fd>>` in the per-process object map. `fork` clones
//! those `Arc`s into the child's object map, which means a pipe created
//! before the fork is transparently shared between parent and child —
//! the classic Unix way for two processes to communicate.
//!
//! Lifetime of the endpoints is tracked by the `Arc` refcount of the
//! shared state: once the last reference to an endpoint is dropped, its
//! `Drop` impl marks the corresponding side as closed and wakes the
//! opposite side. A reader then observes end-of-file (`read` returns 0),
//! a writer observes a broken pipe ([`Errno::Pipe`]).

use alloc::collections::vec_deque::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::future;
use core::task::{Poll, Waker};

use hermit_sync::InterruptTicketMutex;

use crate::errno::Errno;
use crate::fd::{ObjectInterface, PollEvent, StatusFlags};
use crate::io;

/// Capacity of the pipe's ring buffer in bytes.
///
/// Mirrors the 64 KiB default Linux gives a pipe. A write blocks (or, in
/// non-blocking mode, returns [`Errno::Again`]) once the buffer is full.
const PIPE_CAPACITY: usize = 64 * 1024;

/// State shared between the two endpoints of a pipe.
#[derive(Debug)]
struct PipeState {
	/// FIFO byte buffer, capped at [`PIPE_CAPACITY`].
	buffer: VecDeque<u8>,
	/// Set once every read end has been dropped.
	reader_closed: bool,
	/// Set once every write end has been dropped.
	writer_closed: bool,
	/// Tasks blocked in `read`/readable `poll`.
	read_wakers: Vec<Waker>,
	/// Tasks blocked in `write`/writable `poll`.
	write_wakers: Vec<Waker>,
}

impl PipeState {
	fn new() -> Self {
		Self {
			buffer: VecDeque::new(),
			reader_closed: false,
			writer_closed: false,
			read_wakers: Vec::new(),
			write_wakers: Vec::new(),
		}
	}

	/// Wake everyone waiting for the pipe to become readable.
	///
	/// All wakers are drained and woken: readability is level-triggered
	/// and the `poll`-based fd multiplexing re-registers a fresh waker on
	/// each poll, so the queue accumulates stale wakers — waking only one
	/// could wake a dead waker and miss the live one.
	fn wake_readers(&mut self) {
		for waker in self.read_wakers.drain(..) {
			waker.wake();
		}
	}

	/// Wake everyone waiting for the pipe to become writable.
	fn wake_writers(&mut self) {
		for waker in self.write_wakers.drain(..) {
			waker.wake();
		}
	}
}

/// Allocate a fresh pipe, returning its read and write endpoints.
pub(crate) fn pipe() -> (PipeReceiver, PipeSender) {
	let state = Arc::new(InterruptTicketMutex::new(PipeState::new()));
	(
		PipeReceiver {
			state: state.clone(),
			status_flags: StatusFlags::empty(),
		},
		PipeSender {
			state,
			status_flags: StatusFlags::empty(),
		},
	)
}

/// The read end of a pipe.
#[derive(Debug)]
pub(crate) struct PipeReceiver {
	state: Arc<InterruptTicketMutex<PipeState>>,
	status_flags: StatusFlags,
}

impl Drop for PipeReceiver {
	fn drop(&mut self) {
		let mut state = self.state.lock();
		state.reader_closed = true;
		// A blocked writer must return `EPIPE` now that nobody reads.
		state.wake_writers();
	}
}

impl ObjectInterface for PipeReceiver {
	async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
		let nonblock = self.status_flags.contains(StatusFlags::O_NONBLOCK);

		future::poll_fn(|cx| {
			let mut state = self.state.lock();

			if !state.buffer.is_empty() {
				let len = buf.len().min(state.buffer.len());
				for byte in buf.iter_mut().take(len) {
					*byte = state.buffer.pop_front().unwrap();
				}
				// Freed buffer space: a blocked writer can make progress.
				state.wake_writers();
				Poll::Ready(Ok(len))
			} else if state.writer_closed {
				// End of file: all write ends are gone.
				Poll::Ready(Ok(0))
			} else if nonblock {
				Poll::Ready(Err(Errno::Again))
			} else {
				state.read_wakers.push(cx.waker().clone());
				Poll::Pending
			}
		})
		.await
	}

	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		future::poll_fn(|cx| {
			let mut state = self.state.lock();

			let mut available = PollEvent::empty();
			if !state.buffer.is_empty() {
				available.insert(PollEvent::POLLIN | PollEvent::POLLRDNORM);
			}
			if state.writer_closed {
				// EOF is reported as a readable, non-error condition.
				available.insert(PollEvent::POLLIN | PollEvent::POLLRDNORM | PollEvent::POLLHUP);
			}

			let ret = event & available;
			if ret.is_empty() && !state.writer_closed {
				state.read_wakers.push(cx.waker().clone());
				Poll::Pending
			} else {
				Poll::Ready(Ok(ret))
			}
		})
		.await
	}

	async fn status_flags(&self) -> io::Result<StatusFlags> {
		Ok(self.status_flags)
	}

	async fn set_status_flags(&mut self, status_flags: StatusFlags) -> io::Result<()> {
		self.status_flags = status_flags;
		Ok(())
	}
}

/// The write end of a pipe.
#[derive(Debug)]
pub(crate) struct PipeSender {
	state: Arc<InterruptTicketMutex<PipeState>>,
	status_flags: StatusFlags,
}

impl Drop for PipeSender {
	fn drop(&mut self) {
		let mut state = self.state.lock();
		state.writer_closed = true;
		// A blocked reader must observe end-of-file now.
		state.wake_readers();
	}
}

impl ObjectInterface for PipeSender {
	async fn write(&self, buf: &[u8]) -> io::Result<usize> {
		if buf.is_empty() {
			return Ok(0);
		}
		let nonblock = self.status_flags.contains(StatusFlags::O_NONBLOCK);

		future::poll_fn(|cx| {
			let mut state = self.state.lock();

			if state.reader_closed {
				// Writing to a pipe with no readers is a broken pipe.
				return Poll::Ready(Err(Errno::Pipe));
			}

			let free = PIPE_CAPACITY - state.buffer.len();
			if free > 0 {
				let len = buf.len().min(free);
				state.buffer.extend(buf[..len].iter().copied());
				// New data: a blocked reader can make progress.
				state.wake_readers();
				Poll::Ready(Ok(len))
			} else if nonblock {
				Poll::Ready(Err(Errno::Again))
			} else {
				state.write_wakers.push(cx.waker().clone());
				Poll::Pending
			}
		})
		.await
	}

	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		future::poll_fn(|cx| {
			let mut state = self.state.lock();

			let mut available = PollEvent::empty();
			if state.reader_closed {
				// A vanished reader is an error/hangup condition for the writer.
				available.insert(PollEvent::POLLERR);
			} else if state.buffer.len() < PIPE_CAPACITY {
				available.insert(PollEvent::POLLOUT | PollEvent::POLLWRNORM);
			}

			let ret = event & available;
			if ret.is_empty() && !state.reader_closed {
				state.write_wakers.push(cx.waker().clone());
				Poll::Pending
			} else {
				Poll::Ready(Ok(ret))
			}
		})
		.await
	}

	async fn status_flags(&self) -> io::Result<StatusFlags> {
		Ok(self.status_flags)
	}

	async fn set_status_flags(&mut self, status_flags: StatusFlags) -> io::Result<()> {
		self.status_flags = status_flags;
		Ok(())
	}
}
