use core::ffi::c_int;
use core::future;
use core::task::{Context, Poll};

use hermit_sync::SpinMutex;
use smoltcp::socket::udp;
use smoltcp::socket::udp::UdpMetadata;
use smoltcp::wire::{IpEndpoint, Ipv4Address, Ipv6Address};

use crate::errno::Errno;
use crate::executor::block_on;
use crate::executor::network::{Handle, NIC, wake_network_waker};
use crate::fd::{
	self, Endpoint, ListenEndpoint, ObjectInterface, PollEvent, SocketOption, SocketOptionSocket,
};
use crate::io;
use crate::syscalls::socket::Af;

/// Returns the next ephemeral port.
///
/// smoltcp does not check whether a port is already bound by another UDP
/// socket, so the returned port may collide with an explicitly bound one.
fn get_ephemeral_port() -> u16 {
	static LOCAL_ENDPOINT_LOCK: SpinMutex<u16> = SpinMutex::new(49152);

	let mut guard = LOCAL_ENDPOINT_LOCK.lock();
	*guard += 1;
	if *guard == 0 {
		*guard = 1024;
	}
	*guard
}

pub struct Socket {
	handle: Handle,
	is_nonblocking: bool,
	local_endpoint: IpEndpoint,
	remote_endpoint: Option<IpEndpoint>,
}

impl Socket {
	pub fn new(handle: Handle, domain: Af) -> Self {
		let local_endpoint = if domain == Af::Inet {
			IpEndpoint::new(Ipv4Address::UNSPECIFIED.into(), 0)
		} else if domain == Af::Inet6 {
			IpEndpoint::new(Ipv6Address::UNSPECIFIED.into(), 0)
		} else {
			panic!("Unsupported domain for UDP socket: {domain:?}");
		};

		Self {
			handle,
			is_nonblocking: false,
			local_endpoint,
			remote_endpoint: None,
		}
	}

	fn with<R>(&self, f: impl FnOnce(&mut udp::Socket<'_>) -> R) -> R {
		let mut guard = NIC.lock();
		let nic = guard.as_nic_mut().unwrap();
		let r = f(nic.get_mut_socket::<udp::Socket<'_>>(self.handle));
		wake_network_waker();
		r
	}

	async fn close(&self) -> io::Result<()> {
		self.with(|socket| socket.close());
		Ok(())
	}

	async fn write_with_meta(&self, buffer: &[u8], meta: &UdpMetadata) -> io::Result<usize> {
		future::poll_fn(|cx| {
			self.with(|socket| {
				if !socket.is_open() {
					return Poll::Ready(Err(Errno::Io));
				}

				if !socket.can_send() {
					if self.is_nonblocking {
						return Poll::Ready(Err(Errno::Again));
					}
					socket.register_send_waker(cx.waker());
					return Poll::Pending;
				}

				Poll::Ready(
					socket
						.send_slice(buffer, *meta)
						.map(|()| buffer.len())
						.map_err(|_| Errno::Io),
				)
			})
		})
		.await
	}

	/// Returns whether a datagram can be received.
	///
	/// A connected socket drops queued datagrams of other peers.
	fn can_datagram(&self, socket: &mut udp::Socket<'_>) -> bool {
		while let Ok((_, meta)) = socket.peek() {
			if self.remote_endpoint.is_none_or(|ep| ep == meta.endpoint) {
				return true;
			}
			let _ = socket.recv();
		}
		false
	}

	/// Receives a datagram and truncates it to the size of `buf`.
	fn poll_recv(
		&self,
		cx: &mut Context<'_>,
		buf: &mut [u8],
	) -> Poll<io::Result<(usize, IpEndpoint)>> {
		self.with(|socket| {
			if !socket.is_open() {
				return Poll::Ready(Err(Errno::Io));
			}

			if !self.can_datagram(socket) {
				if self.is_nonblocking {
					return Poll::Ready(Err(Errno::Again));
				}
				socket.register_recv_waker(cx.waker());
				return Poll::Pending;
			}

			let Ok((data, meta)) = socket.recv() else {
				return Poll::Ready(Err(Errno::Io));
			};
			let len = data.len().min(buf.len());
			buf[..len].copy_from_slice(&data[..len]);
			Poll::Ready(Ok((len, meta.endpoint)))
		})
	}
}

impl ObjectInterface for Socket {
	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		future::poll_fn(|cx| {
			self.with(|socket| {
				let ret = if socket.is_open() {
					let mut avail = PollEvent::empty();

					if socket.can_send() {
						avail.insert(
							PollEvent::POLLOUT | PollEvent::POLLWRNORM | PollEvent::POLLWRBAND,
						);
					}

					if self.can_datagram(socket) {
						avail.insert(
							PollEvent::POLLIN | PollEvent::POLLRDNORM | PollEvent::POLLRDBAND,
						);
					}

					event & avail
				} else {
					PollEvent::POLLNVAL
				};

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
			})
		})
		.await
	}

	async fn bind(&mut self, endpoint: ListenEndpoint) -> io::Result<()> {
		#[allow(irrefutable_let_patterns)]
		let ListenEndpoint::Ip(endpoint) = endpoint else {
			return Err(Errno::Io);
		};

		let mut endpoint = endpoint;
		if endpoint.port == 0 {
			endpoint.port = get_ephemeral_port();
		}

		self.local_endpoint.port = endpoint.port;
		if let Some(addr) = endpoint.addr {
			self.local_endpoint.addr = addr;
		}
		self.with(|socket| socket.bind(endpoint).map_err(|_| Errno::Addrinuse))
	}

	async fn connect(&mut self, endpoint: Endpoint) -> io::Result<()> {
		#[allow(irrefutable_let_patterns)]
		let Endpoint::Ip(endpoint) = endpoint else {
			return Err(Errno::Io);
		};

		self.remote_endpoint = Some(endpoint);
		Ok(())
	}

	async fn sendto(&self, buf: &[u8], endpoint: Endpoint) -> io::Result<usize> {
		#[allow(irrefutable_let_patterns)]
		let Endpoint::Ip(endpoint) = endpoint else {
			return Err(Errno::Io);
		};

		let meta = UdpMetadata::from(endpoint);
		self.write_with_meta(buf, &meta).await
	}

	async fn recvfrom(&self, buf: &mut [u8]) -> io::Result<(usize, Endpoint)> {
		future::poll_fn(|cx| self.poll_recv(cx, buf))
			.await
			.map(|(len, endpoint)| (len, Endpoint::Ip(endpoint)))
	}

	async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
		future::poll_fn(|cx| self.poll_recv(cx, buf))
			.await
			.map(|(len, _)| len)
	}

	async fn write(&self, buf: &[u8]) -> io::Result<usize> {
		let endpoint = self.remote_endpoint.ok_or(Errno::Inval)?;

		let meta = UdpMetadata::from(endpoint);
		self.write_with_meta(buf, &meta).await
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

	async fn getsockname(&self) -> io::Result<Option<Endpoint>> {
		Ok(Some(Endpoint::Ip(self.local_endpoint)))
	}

	async fn getsockopt(&self, opt: SocketOption) -> io::Result<c_int> {
		let mut guard = NIC.lock();
		let socket = guard
			.as_nic_mut()
			.unwrap()
			.get_mut_socket::<udp::Socket<'_>>(self.handle);

		match opt {
			SocketOption::SocketOption(SocketOptionSocket::SoSndbuf) => {
				Ok(c_int::try_from(socket.payload_send_capacity()).unwrap())
			}
			SocketOption::SocketOption(SocketOptionSocket::SoRcvbuf) => {
				Ok(c_int::try_from(socket.payload_recv_capacity()).unwrap())
			}
			_ => Err(Errno::Inval),
		}
	}
}

impl Drop for Socket {
	fn drop(&mut self) {
		let _ = block_on(self.close(), None);
		NIC.lock().as_nic_mut().unwrap().destroy_socket(self.handle);
	}
}
