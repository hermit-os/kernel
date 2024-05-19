use alloc::boxed::Box;
use core::future;
use core::ops::DerefMut;
use core::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use core::task::Poll;

use async_trait::async_trait;
use smoltcp::iface;
use smoltcp::socket::tcp;
use smoltcp::time::Duration;
use smoltcp::wire::{IpEndpoint, IpListenEndpoint};

use crate::executor::block_on;
use crate::executor::network::{now, Handle, NetworkState, NIC};
use crate::fd::{IoCtl, IoError, ObjectInterface, PollEvent, SocketOption};
use crate::DEFAULT_KEEP_ALIVE_INTERVAL;

/// further receives will be disallowed
pub const SHUT_RD: i32 = 0;
/// further sends will be disallowed
pub const SHUT_WR: i32 = 1;
/// further sends and receives will be disallowed
pub const SHUT_RDWR: i32 = 2;

fn get_ephemeral_port() -> u16 {
	static LOCAL_ENDPOINT: AtomicU16 = AtomicU16::new(49152);

	LOCAL_ENDPOINT.fetch_add(1, Ordering::SeqCst)
}

#[derive(Debug)]
pub struct Socket {
	handle: Handle,
	port: AtomicU16,
	nonblocking: AtomicBool,
	listen: AtomicBool,
}

impl Socket {
	pub fn new(handle: Handle) -> Self {
		Self {
			handle,
			port: AtomicU16::new(0),
			nonblocking: AtomicBool::new(false),
			listen: AtomicBool::new(false),
		}
	}

	fn with<R>(&self, f: impl FnOnce(&mut tcp::Socket<'_>) -> R) -> R {
		let mut guard = NIC.lock();
		let nic = guard.as_nic_mut().unwrap();
		let result = f(nic.get_mut_socket::<tcp::Socket<'_>>(self.handle));
		nic.poll_common(now());

		result
	}

	fn with_context<R>(&self, f: impl FnOnce(&mut tcp::Socket<'_>, &mut iface::Context) -> R) -> R {
		let mut guard = NIC.lock();
		let nic = guard.as_nic_mut().unwrap();
		let (s, cx) = nic.get_socket_and_context::<tcp::Socket<'_>>(self.handle);
		let result = f(s, cx);
		nic.poll_common(now());

		result
	}

	async fn async_connect(&self, endpoint: IpEndpoint) -> Result<(), IoError> {
		self.with_context(|socket, cx| socket.connect(cx, endpoint, get_ephemeral_port()))
			.map_err(|_| IoError::EIO)?;

		future::poll_fn(|cx| {
			self.with(|socket| match socket.state() {
				tcp::State::Closed | tcp::State::TimeWait => Poll::Ready(Err(IoError::EFAULT)),
				tcp::State::Listen => Poll::Ready(Err(IoError::EIO)),
				tcp::State::SynSent | tcp::State::SynReceived => {
					socket.register_send_waker(cx.waker());
					Poll::Pending
				}
				_ => Poll::Ready(Ok(())),
			})
		})
		.await
	}

	async fn async_close(&self) -> Result<(), IoError> {
		future::poll_fn(|cx| {
			self.with(|socket| match socket.state() {
				tcp::State::FinWait1
				| tcp::State::FinWait2
				| tcp::State::Closed
				| tcp::State::Closing
				| tcp::State::TimeWait => Poll::Ready(Err(IoError::EIO)),
				_ => {
					if socket.send_queue() > 0 {
						socket.register_send_waker(cx.waker());
						Poll::Pending
					} else {
						socket.close();
						Poll::Ready(Ok(()))
					}
				}
			})
		})
		.await?;

		future::poll_fn(|cx| {
			self.with(|socket| match socket.state() {
				tcp::State::FinWait1
				| tcp::State::FinWait2
				| tcp::State::Closed
				| tcp::State::Closing
				| tcp::State::TimeWait => Poll::Ready(Ok(())),
				_ => {
					socket.register_send_waker(cx.waker());
					Poll::Pending
				}
			})
		})
		.await
	}

	async fn async_accept(&self) -> Result<IpEndpoint, IoError> {
		future::poll_fn(|cx| {
			self.with(|socket| match socket.state() {
				tcp::State::Closed => {
					let _ = socket.listen(self.port.load(Ordering::Acquire));
					Poll::Ready(())
				}
				tcp::State::Listen | tcp::State::Established => Poll::Ready(()),
				_ => {
					socket.register_recv_waker(cx.waker());
					Poll::Pending
				}
			})
		})
		.await;

		future::poll_fn(|cx| {
			self.with(|socket| {
				if socket.is_active() {
					Poll::Ready(Ok(()))
				} else {
					match socket.state() {
						tcp::State::Closed
						| tcp::State::Closing
						| tcp::State::FinWait1
						| tcp::State::FinWait2 => Poll::Ready(Err(IoError::EIO)),
						_ => {
							socket.register_recv_waker(cx.waker());
							Poll::Pending
						}
					}
				}
			})
		})
		.await?;

		let mut guard = NIC.lock();
		let nic = guard.as_nic_mut().map_err(|_| IoError::EIO)?;
		let socket = nic.get_mut_socket::<tcp::Socket<'_>>(self.handle);
		socket.set_keep_alive(Some(Duration::from_millis(DEFAULT_KEEP_ALIVE_INTERVAL)));

		Ok(socket.remote_endpoint().unwrap())
	}
}

#[async_trait]
impl ObjectInterface for Socket {
	async fn poll(&self, event: PollEvent) -> Result<PollEvent, IoError> {
		future::poll_fn(|cx| {
			self.with(|socket| match socket.state() {
				tcp::State::Closed | tcp::State::Closing | tcp::State::CloseWait => {
					let available = PollEvent::POLLOUT
						| PollEvent::POLLWRNORM | PollEvent::POLLWRBAND
						| PollEvent::POLLIN | PollEvent::POLLRDNORM
						| PollEvent::POLLRDBAND;

					let ret = event & available;

					if ret.is_empty() {
						Poll::Ready(Ok(PollEvent::POLLHUP))
					} else {
						Poll::Ready(Ok(ret))
					}
				}
				tcp::State::FinWait1 | tcp::State::FinWait2 | tcp::State::TimeWait => {
					Poll::Ready(Ok(PollEvent::POLLHUP))
				}
				tcp::State::Listen => {
					socket.register_recv_waker(cx.waker());
					socket.register_send_waker(cx.waker());
					Poll::Pending
				}
				_ => {
					let mut available = PollEvent::empty();

					if socket.can_recv()
						|| socket.may_recv() && self.listen.swap(false, Ordering::Relaxed)
					{
						// In case, we just establish a fresh connection in non-blocking mode, we try to read data.
						available.insert(
							PollEvent::POLLIN | PollEvent::POLLRDNORM | PollEvent::POLLRDBAND,
						);
					}

					if socket.can_send() {
						available.insert(
							PollEvent::POLLOUT | PollEvent::POLLWRNORM | PollEvent::POLLWRBAND,
						);
					}

					let ret = event & available;

					if ret.is_empty() {
						if event.intersects(
							PollEvent::POLLIN | PollEvent::POLLRDNORM | PollEvent::POLLRDBAND,
						) {
							socket.register_recv_waker(cx.waker());
						}

						if event.intersects(
							PollEvent::POLLOUT | PollEvent::POLLWRNORM | PollEvent::POLLWRBAND,
						) {
							socket.register_send_waker(cx.waker());
						}

						Poll::Pending
					} else {
						Poll::Ready(Ok(ret))
					}
				}
			})
		})
		.await
	}

	// TODO: Remove allow once fixed:
	// https://github.com/rust-lang/rust-clippy/issues/11380
	#[allow(clippy::needless_pass_by_ref_mut)]
	async fn async_read(&self, buffer: &mut [u8]) -> Result<usize, IoError> {
		future::poll_fn(|cx| {
			self.with(|socket| match socket.state() {
				tcp::State::Closed | tcp::State::Closing | tcp::State::CloseWait => {
					Poll::Ready(Ok(0))
				}
				tcp::State::FinWait1
				| tcp::State::FinWait2
				| tcp::State::Listen
				| tcp::State::TimeWait => Poll::Ready(Err(IoError::EIO)),
				_ => {
					if socket.can_recv() {
						Poll::Ready(
							socket
								.recv(|data| {
									let len = core::cmp::min(buffer.len(), data.len());
									buffer[..len].copy_from_slice(&data[..len]);
									(len, len)
								})
								.map_err(|_| IoError::EIO),
						)
					} else {
						socket.register_recv_waker(cx.waker());
						Poll::Pending
					}
				}
			})
		})
		.await
	}

	async fn async_write(&self, buffer: &[u8]) -> Result<usize, IoError> {
		let mut pos: usize = 0;

		while pos < buffer.len() {
			let n = future::poll_fn(|cx| {
				self.with(|socket| {
					match socket.state() {
						tcp::State::Closed | tcp::State::Closing | tcp::State::CloseWait => {
							Poll::Ready(Ok(0))
						}
						tcp::State::FinWait1
						| tcp::State::FinWait2
						| tcp::State::Listen
						| tcp::State::TimeWait => Poll::Ready(Err(IoError::EIO)),
						_ => {
							if socket.can_send() {
								Poll::Ready(
									socket.send_slice(&buffer[pos..]).map_err(|_| IoError::EIO),
								)
							} else if pos > 0 {
								// we already send some data => return 0 as signal to stop the
								// async write
								Poll::Ready(Ok(0))
							} else {
								socket.register_send_waker(cx.waker());
								Poll::Pending
							}
						}
					}
				})
			})
			.await?;

			if n == 0 {
				break;
			}

			pos += n;
		}

		Ok(pos)
	}

	fn bind(&self, endpoint: IpListenEndpoint) -> Result<(), IoError> {
		self.port.store(endpoint.port, Ordering::Release);
		Ok(())
	}

	fn connect(&self, endpoint: IpEndpoint) -> Result<(), IoError> {
		if self.nonblocking.load(Ordering::Acquire) {
			block_on(self.async_connect(endpoint), Some(Duration::ZERO.into())).map_err(|x| {
				if x == IoError::ETIME {
					IoError::EAGAIN
				} else {
					x
				}
			})
		} else {
			block_on(self.async_connect(endpoint), None)
		}
	}

	fn accept(&self) -> Result<IpEndpoint, IoError> {
		if self.is_nonblocking() {
			block_on(self.async_accept(), Some(Duration::ZERO.into())).map_err(|x| {
				if x == IoError::ETIME {
					IoError::EAGAIN
				} else {
					x
				}
			})
		} else {
			block_on(self.async_accept(), None)
		}
	}

	fn getpeername(&self) -> Option<IpEndpoint> {
		self.with(|socket| socket.remote_endpoint())
	}

	fn getsockname(&self) -> Option<IpEndpoint> {
		self.with(|socket| socket.local_endpoint())
	}

	fn is_nonblocking(&self) -> bool {
		self.nonblocking.load(Ordering::Acquire)
	}

	fn listen(&self, _backlog: i32) -> Result<(), IoError> {
		self.with(|socket| {
			if !socket.is_open() {
				self.listen.store(true, Ordering::Relaxed);
				socket
					.listen(self.port.load(Ordering::Acquire))
					.map(|_| ())
					.map_err(|_| IoError::EIO)
			} else {
				Err(IoError::EIO)
			}
		})
	}

	fn setsockopt(&self, opt: SocketOption, optval: bool) -> Result<(), IoError> {
		if opt == SocketOption::TcpNoDelay {
			self.with(|socket| {
				socket.set_nagle_enabled(optval);
				if optval {
					socket.set_ack_delay(None);
				} else {
					socket.set_ack_delay(Some(Duration::from_millis(10)));
				}
			});
			Ok(())
		} else {
			Err(IoError::EINVAL)
		}
	}

	fn getsockopt(&self, opt: SocketOption) -> Result<bool, IoError> {
		if opt == SocketOption::TcpNoDelay {
			self.with(|socket| Ok(socket.nagle_enabled()))
		} else {
			Err(IoError::EINVAL)
		}
	}

	fn shutdown(&self, how: i32) -> Result<(), IoError> {
		match how {
			SHUT_RD /* Read  */ |
			SHUT_WR /* Write */ |
			SHUT_RDWR /* Both */ => Ok(()),
			_ => Err(IoError::EINVAL),
		}
	}

	fn ioctl(&self, cmd: IoCtl, value: bool) -> Result<(), IoError> {
		if cmd == IoCtl::NonBlocking {
			if value {
				trace!("set device to nonblocking mode");
				self.nonblocking.store(true, Ordering::Release);
			} else {
				trace!("set device to blocking mode");
				self.nonblocking.store(false, Ordering::Release);
			}

			Ok(())
		} else {
			Err(IoError::EINVAL)
		}
	}
}

impl Clone for Socket {
	fn clone(&self) -> Self {
		let mut guard = NIC.lock();

		let handle = if let NetworkState::Initialized(nic) = guard.deref_mut() {
			nic.create_tcp_handle().unwrap()
		} else {
			panic!("Unable to create handle");
		};

		drop(guard);
		let port = self.port.load(Ordering::Acquire);
		let obj = Self {
			handle,
			port: AtomicU16::new(port),
			nonblocking: AtomicBool::new(self.nonblocking.load(Ordering::Acquire)),
			listen: AtomicBool::new(false),
		};

		if port > 0 {
			let _ = obj.listen(1024);
		}

		obj
	}
}

impl Drop for Socket {
	fn drop(&mut self) {
		let _ = block_on(self.async_close(), None);
		NIC.lock().as_nic_mut().unwrap().destroy_socket(self.handle);
	}
}
