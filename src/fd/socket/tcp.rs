use alloc::boxed::Box;
use alloc::collections::BTreeSet;
use alloc::sync::Arc;
use core::future;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU16, Ordering};
use core::task::Poll;

use async_trait::async_trait;
use smoltcp::iface;
use smoltcp::socket::tcp;
use smoltcp::time::Duration;
use smoltcp::wire::{IpEndpoint, Ipv4Address, Ipv6Address};

use crate::executor::block_on;
use crate::executor::network::{Handle, NIC};
use crate::fd::{self, Endpoint, ListenEndpoint, ObjectInterface, PollEvent, SocketOption};
use crate::syscalls::socket::{AF_INET, AF_INET6};
use crate::{DEFAULT_KEEP_ALIVE_INTERVAL, io};

/// further receives will be disallowed
pub const SHUT_RD: i32 = 0;
/// further sends will be disallowed
pub const SHUT_WR: i32 = 1;
/// further sends and receives will be disallowed
pub const SHUT_RDWR: i32 = 2;
/// The default queue size for incoming connections
pub const DEFAULT_BACKLOG: i32 = 128;

fn get_ephemeral_port() -> u16 {
	static LOCAL_ENDPOINT: AtomicU16 = AtomicU16::new(49152);

	LOCAL_ENDPOINT.fetch_add(1, Ordering::SeqCst)
}

#[derive(Debug)]
pub struct Socket {
	handle: BTreeSet<Handle>,
	endpoint: IpEndpoint,
	is_nonblocking: bool,
	is_listen: bool,
}

impl Socket {
	pub fn new(h: Handle, domain: i32) -> Self {
		let mut handle = BTreeSet::new();
		handle.insert(h);

		let endpoint = if domain == AF_INET {
			IpEndpoint::new(Ipv4Address::UNSPECIFIED.into(), 0)
		} else if domain == AF_INET6 {
			IpEndpoint::new(Ipv6Address::UNSPECIFIED.into(), 0)
		} else {
			panic!("Unsupported domain for TCP socket: {}", domain);
		};

		Self {
			handle,
			endpoint,
			is_nonblocking: false,
			is_listen: false,
		}
	}

	fn with<R>(&self, f: impl FnOnce(&mut tcp::Socket<'_>) -> R) -> R {
		let mut guard = NIC.lock();
		let nic = guard.as_nic_mut().unwrap();
		f(nic.get_mut_socket::<tcp::Socket<'_>>(*self.handle.first().unwrap()))
	}

	fn with_context<R>(&self, f: impl FnOnce(&mut tcp::Socket<'_>, &mut iface::Context) -> R) -> R {
		let mut guard = NIC.lock();
		let nic = guard.as_nic_mut().unwrap();
		let (s, cx) = nic.get_socket_and_context::<tcp::Socket<'_>>(*self.handle.first().unwrap());
		f(s, cx)
	}

	async fn close(&self) -> io::Result<()> {
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

		if self.handle.len() > 1 {
			let mut guard = NIC.lock();
			let nic = guard.as_nic_mut().unwrap();

			for handle in self.handle.iter().skip(1) {
				let socket = nic.get_mut_socket::<tcp::Socket<'_>>(*handle);
				if socket.is_active() {
					socket.close();
				}
			}
		}

		future::poll_fn(|cx| {
			self.with(|socket| {
				if socket.is_active() {
					socket.register_send_waker(cx.waker());
					socket.register_recv_waker(cx.waker());
					Poll::Pending
				} else {
					Poll::Ready(Ok(()))
				}
			})
		})
		.await
	}

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

					if socket.can_recv() || socket.may_recv() && self.is_listen {
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

	async fn read(&self, buffer: &mut [MaybeUninit<u8>]) -> io::Result<usize> {
		future::poll_fn(|cx| {
			self.with(|socket| {
				let state = socket.state();
				match state {
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
										buffer[..len].write_copy_of_slice(&data[..len]);
										(len, len)
									})
									.map_err(|_| io::Error::EIO),
							)
						} else if state == tcp::State::CloseWait {
							// The local end-point has received a connection termination request
							// and not data are in the receive buffer => return 0 to close the connection
							Poll::Ready(Ok(0))
						} else if self.is_nonblocking {
							Poll::Ready(Err(io::Error::EAGAIN))
						} else {
							socket.register_recv_waker(cx.waker());
							Poll::Pending
						}
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
							} else if self.is_nonblocking {
								Poll::Ready(Err(io::Error::EAGAIN))
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

	async fn bind(&mut self, endpoint: ListenEndpoint) -> io::Result<()> {
		#[allow(irrefutable_let_patterns)]
		if let ListenEndpoint::Ip(endpoint) = endpoint {
			self.endpoint.port = endpoint.port;
			if let Some(addr) = endpoint.addr {
				self.endpoint.addr = addr;
			}
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

	async fn accept(&mut self) -> io::Result<(Socket, Endpoint)> {
		if !self.is_listen {
			self.listen(DEFAULT_BACKLOG).await?;
		}

		let connection_handle = future::poll_fn(|cx| {
			let mut guard = NIC.lock();
			let nic = guard.as_nic_mut().unwrap();
			let mut socket_handle = None;

			for handle in self.handle.iter() {
				let s = nic.get_mut_socket::<tcp::Socket<'_>>(*handle);

				if s.is_active() {
					socket_handle = Some(*handle);
					break;
				}
			}

			if let Some(handle) = socket_handle {
				self.handle.remove(&handle);
				Poll::Ready(Ok(handle))
			} else if self.is_nonblocking {
				Poll::Ready(Err(io::Error::EAGAIN))
			} else {
				for handle in self.handle.iter() {
					let s = nic.get_mut_socket::<tcp::Socket<'_>>(*handle);
					s.register_recv_waker(cx.waker());
				}

				Poll::Pending
			}
		})
		.await?;

		let mut guard = NIC.lock();
		let nic = guard.as_nic_mut().map_err(|_| io::Error::EIO)?;
		let socket = nic.get_mut_socket::<tcp::Socket<'_>>(connection_handle);
		socket.set_keep_alive(Some(Duration::from_millis(DEFAULT_KEEP_ALIVE_INTERVAL)));
		let endpoint = Endpoint::Ip(socket.remote_endpoint().unwrap());
		let nagle_enabled = socket.nagle_enabled();

		// fill up queue for pending connections
		let new_handle = nic.create_tcp_handle().unwrap();
		self.handle.insert(new_handle);
		let socket = nic.get_mut_socket::<tcp::Socket<'_>>(new_handle);
		socket.set_nagle_enabled(nagle_enabled);
		socket
			.listen(self.endpoint.port)
			.map_err(|_| io::Error::EIO)?;

		let mut handle = BTreeSet::new();
		handle.insert(connection_handle);

		let socket = Socket {
			handle,
			endpoint: self.endpoint,
			is_nonblocking: self.is_nonblocking,
			is_listen: false,
		};

		Ok((socket, endpoint))
	}

	async fn getpeername(&self) -> io::Result<Option<Endpoint>> {
		Ok(self
			.with(|socket| socket.remote_endpoint())
			.map(Endpoint::Ip))
	}

	async fn getsockname(&self) -> io::Result<Option<Endpoint>> {
		Ok(self
			.with(|socket| {
				if let Some(endpoint) = socket.local_endpoint() {
					Some(endpoint)
				} else {
					Some(self.endpoint)
				}
			})
			.map(Endpoint::Ip))
	}

	async fn listen(&mut self, backlog: i32) -> io::Result<()> {
		let nagle_enabled = self.with(|socket| socket.nagle_enabled());
		let mut guard = NIC.lock();
		let nic = guard.as_nic_mut().unwrap();

		let socket = nic.get_mut_socket::<tcp::Socket<'_>>(*self.handle.first().unwrap());

		if socket.is_open() {
			return Err(io::Error::EIO);
		}

		if backlog <= 0 {
			return Err(io::Error::EINVAL);
		}

		socket
			.listen(self.endpoint.port)
			.map_err(|_| io::Error::EIO)?;

		self.is_listen = true;

		for _ in 1..backlog {
			let handle = nic.create_tcp_handle().unwrap();

			let s = nic.get_mut_socket::<tcp::Socket<'_>>(handle);
			s.set_nagle_enabled(nagle_enabled);
			s.listen(self.endpoint.port).map_err(|_| io::Error::EIO)?;

			self.handle.insert(handle);
		}

		Ok(())
	}

	async fn setsockopt(&self, opt: SocketOption, optval: bool) -> io::Result<()> {
		if opt == SocketOption::TcpNoDelay {
			let mut guard = NIC.lock();
			let nic = guard.as_nic_mut().unwrap();

			for i in self.handle.iter() {
				let socket = nic.get_mut_socket::<tcp::Socket<'_>>(*i);
				socket.set_nagle_enabled(optval);
			}

			Ok(())
		} else {
			Err(io::Error::EINVAL)
		}
	}

	async fn getsockopt(&self, opt: SocketOption) -> io::Result<bool> {
		if opt == SocketOption::TcpNoDelay {
			let mut guard = NIC.lock();
			let nic = guard.as_nic_mut().unwrap();
			let socket = nic.get_mut_socket::<tcp::Socket<'_>>(*self.handle.first().unwrap());

			Ok(socket.nagle_enabled())
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

	async fn status_flags(&self) -> io::Result<fd::StatusFlags> {
		let status_flags = if self.is_nonblocking {
			fd::StatusFlags::O_NONBLOCK
		} else {
			fd::StatusFlags::empty()
		};

		Ok(status_flags)
	}

	async fn set_status_flags(&mut self, status_flags: fd::StatusFlags) -> io::Result<()> {
		self.is_nonblocking = status_flags.contains(fd::StatusFlags::O_NONBLOCK);
		Ok(())
	}
}

impl Drop for Socket {
	fn drop(&mut self) {
		let _ = block_on(self.close(), None);

		let mut guard = NIC.lock();
		for h in self.handle.iter() {
			guard.as_nic_mut().unwrap().destroy_socket(*h);
		}
	}
}

#[async_trait]
impl ObjectInterface for async_lock::RwLock<Socket> {
	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		self.read().await.poll(event).await
	}

	async fn read(&self, buffer: &mut [MaybeUninit<u8>]) -> io::Result<usize> {
		self.read().await.read(buffer).await
	}

	async fn write(&self, buffer: &[u8]) -> io::Result<usize> {
		self.read().await.write(buffer).await
	}

	async fn bind(&self, endpoint: ListenEndpoint) -> io::Result<()> {
		self.write().await.bind(endpoint).await
	}

	async fn connect(&self, endpoint: Endpoint) -> io::Result<()> {
		self.read().await.connect(endpoint).await
	}

	async fn accept(&self) -> io::Result<(Arc<dyn ObjectInterface>, Endpoint)> {
		let (socket, endpoint) = self.write().await.accept().await?;
		Ok((Arc::new(async_lock::RwLock::new(socket)), endpoint))
	}

	async fn getpeername(&self) -> io::Result<Option<Endpoint>> {
		self.read().await.getpeername().await
	}

	async fn getsockname(&self) -> io::Result<Option<Endpoint>> {
		self.read().await.getsockname().await
	}

	async fn listen(&self, backlog: i32) -> io::Result<()> {
		self.write().await.listen(backlog).await
	}

	async fn setsockopt(&self, opt: SocketOption, optval: bool) -> io::Result<()> {
		self.read().await.setsockopt(opt, optval).await
	}

	async fn getsockopt(&self, opt: SocketOption) -> io::Result<bool> {
		self.read().await.getsockopt(opt).await
	}

	async fn shutdown(&self, how: i32) -> io::Result<()> {
		self.read().await.shutdown(how).await
	}

	async fn status_flags(&self) -> io::Result<fd::StatusFlags> {
		self.read().await.status_flags().await
	}

	async fn set_status_flags(&self, status_flags: fd::StatusFlags) -> io::Result<()> {
		self.write().await.set_status_flags(status_flags).await
	}
}
