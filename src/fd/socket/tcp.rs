use alloc::boxed::Box;
use core::future;
use core::ops::DerefMut;
use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, Ordering};
use core::task::Poll;

use async_trait::async_trait;
use smoltcp::iface;
use smoltcp::socket::tcp;
use smoltcp::time::Duration;

use crate::executor::block_on;
use crate::executor::network::{now, Handle, NetworkState, NIC};
use crate::fd::{Endpoint, IoCtl, ListenEndpoint, ObjectInterface, PollEvent, SocketOption};
use crate::{io, DEFAULT_KEEP_ALIVE_INTERVAL};

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
	backlog: AtomicU32,
	nonblocking: AtomicBool,
}

impl Socket {
	pub fn new(handle: Handle) -> Self {
		Self {
			handle,
			port: AtomicU16::new(0),
			backlog: AtomicU32::new(0),
			nonblocking: AtomicBool::new(false),
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

	async fn async_close(&self) -> io::Result<()> {
		future::poll_fn(|_cx| {
			self.with(|socket| {
				if socket.is_active() {
					socket.close();
					Poll::Ready(Ok(()))
				} else {
					Poll::Ready(Err(io::Error::EIO))
				}
			})
		})
		.await?;

		future::poll_fn(|cx| {
			self.with(|socket| {
				if !socket.is_active() {
					Poll::Ready(Ok(()))
				} else {
					socket.register_send_waker(cx.waker());
					socket.register_recv_waker(cx.waker());
					Poll::Pending
				}
			})
		})
		.await
	}
}

#[async_trait]
impl ObjectInterface for Socket {
	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		future::poll_fn(|cx| {
			self.with(|socket| match socket.state() {
				tcp::State::Closed | tcp::State::Closing | tcp::State::CloseWait => {
					let available = PollEvent::POLLOUT
						| PollEvent::POLLWRNORM
						| PollEvent::POLLWRBAND
						| PollEvent::POLLIN
						| PollEvent::POLLRDNORM
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
						|| socket.may_recv() && self.backlog.load(Ordering::Acquire) > 0
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
	async fn read(&self, buffer: &mut [u8]) -> io::Result<usize> {
		future::poll_fn(|cx| {
			self.with(|socket| match socket.state() {
				tcp::State::Closed => Poll::Ready(Ok(0)),
				tcp::State::FinWait1
				| tcp::State::FinWait2
				| tcp::State::Listen
				| tcp::State::TimeWait => Poll::Ready(Err(io::Error::EIO)),
				_ => {
					if socket.can_recv() {
						Poll::Ready(
							socket
								.recv(|data| {
									let len = core::cmp::min(buffer.len(), data.len());
									buffer[..len].copy_from_slice(&data[..len]);
									(len, len)
								})
								.map_err(|_| io::Error::EIO),
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

	async fn write(&self, buffer: &[u8]) -> io::Result<usize> {
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
						| tcp::State::TimeWait => Poll::Ready(Err(io::Error::EIO)),
						_ => {
							if socket.can_send() {
								Poll::Ready(
									socket
										.send_slice(&buffer[pos..])
										.map_err(|_| io::Error::EIO),
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

	async fn bind(&self, endpoint: ListenEndpoint) -> io::Result<()> {
		#[allow(irrefutable_let_patterns)]
		if let ListenEndpoint::Ip(endpoint) = endpoint {
			self.port.store(endpoint.port, Ordering::Release);
			Ok(())
		} else {
			Err(io::Error::EIO)
		}
	}

	async fn connect(&self, endpoint: Endpoint) -> io::Result<()> {
		#[allow(irrefutable_let_patterns)]
		if let Endpoint::Ip(endpoint) = endpoint {
			self.with_context(|socket, cx| socket.connect(cx, endpoint, get_ephemeral_port()))
				.map_err(|_| io::Error::EIO)?;

			future::poll_fn(|cx| {
				self.with(|socket| match socket.state() {
					tcp::State::Closed | tcp::State::TimeWait => {
						Poll::Ready(Err(io::Error::EFAULT))
					}
					tcp::State::Listen => Poll::Ready(Err(io::Error::EIO)),
					tcp::State::SynSent | tcp::State::SynReceived => {
						socket.register_send_waker(cx.waker());
						Poll::Pending
					}
					_ => Poll::Ready(Ok(())),
				})
			})
			.await
		} else {
			Err(io::Error::EIO)
		}
	}

	async fn accept(&self) -> io::Result<Endpoint> {
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
						| tcp::State::FinWait2 => Poll::Ready(Err(io::Error::EIO)),
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
		let nic = guard.as_nic_mut().map_err(|_| io::Error::EIO)?;
		let socket = nic.get_mut_socket::<tcp::Socket<'_>>(self.handle);
		socket.set_keep_alive(Some(Duration::from_millis(DEFAULT_KEEP_ALIVE_INTERVAL)));

		Ok(Endpoint::Ip(socket.remote_endpoint().unwrap()))
	}

	async fn getpeername(&self) -> io::Result<Option<Endpoint>> {
		Ok(self
			.with(|socket| socket.remote_endpoint())
			.map(Endpoint::Ip))
	}

	async fn getsockname(&self) -> io::Result<Option<Endpoint>> {
		Ok(self
			.with(|socket| socket.local_endpoint())
			.map(Endpoint::Ip))
	}

	async fn is_nonblocking(&self) -> io::Result<bool> {
		Ok(self.nonblocking.load(Ordering::Acquire))
	}

	async fn listen(&self, backlog: i32) -> io::Result<()> {
		self.with(|socket| {
			if !socket.is_open() {
				if backlog > 0 {
					self.backlog
						.store(backlog.try_into().unwrap(), Ordering::Relaxed);

					socket
						.listen(self.port.load(Ordering::Acquire))
						.map(|_| ())
						.map_err(|_| io::Error::EIO)?;

					let rx_size = socket.recv_queue();
					let tx_size = socket.send_queue();
					let is_nagle = socket.nagle_enabled();
					for _ in 1..backlog {
						let rx_buffer = tcp::SocketBuffer::new(vec![0; rx_size]);
						let tx_buffer = tcp::SocketBuffer::new(vec![0; tx_size]);
						let mut tcp_socket = tcp::Socket::new(rx_buffer, tx_buffer);
						tcp_socket.set_nagle_enabled(is_nagle);
					}

					Ok(())
				} else {
					Err(io::Error::EINVAL)
				}
			} else {
				Err(io::Error::EIO)
			}
		})
	}

	async fn setsockopt(&self, opt: SocketOption, optval: bool) -> io::Result<()> {
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
			Err(io::Error::EINVAL)
		}
	}

	async fn getsockopt(&self, opt: SocketOption) -> io::Result<bool> {
		if opt == SocketOption::TcpNoDelay {
			self.with(|socket| Ok(socket.nagle_enabled()))
		} else {
			Err(io::Error::EINVAL)
		}
	}

	async fn shutdown(&self, how: i32) -> io::Result<()> {
		match how {
			SHUT_RD /* Read  */ |
			SHUT_WR /* Write */ |
			SHUT_RDWR /* Both */ => Ok(()),
			_ => Err(io::Error::EINVAL),
		}
	}

	async fn ioctl(&self, cmd: IoCtl, value: bool) -> io::Result<()> {
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
			Err(io::Error::EINVAL)
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
		let backlog = self.backlog.load(Ordering::Acquire);
		let obj = Self {
			handle,
			port: AtomicU16::new(port),
			backlog: AtomicU32::new(backlog),
			nonblocking: AtomicBool::new(self.nonblocking.load(Ordering::Acquire)),
		};

		if port > 0 {
			let _ = block_on(obj.listen(backlog.try_into().unwrap()), None);
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
