use alloc::boxed::Box;
use core::future;
use core::mem::MaybeUninit;
use core::task::Poll;

use async_trait::async_trait;
use smoltcp::socket::udp;
use smoltcp::socket::udp::UdpMetadata;
use smoltcp::wire::{IpEndpoint, Ipv4Address, Ipv6Address};

use crate::errno::Errno;
use crate::executor::block_on;
use crate::executor::network::{Handle, NIC};
use crate::fd::{self, Endpoint, ListenEndpoint, ObjectInterface, PollEvent};
use crate::io;
use crate::syscalls::socket::Af;

pub struct Socket {
	handle: Handle,
	nonblocking: bool,
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
			panic!("Unsupported domain for TCP socket: {domain:?}");
		};

		Self {
			handle,
			nonblocking: false,
			local_endpoint,
			remote_endpoint: None,
		}
	}

	fn with<R>(&self, f: impl FnOnce(&mut udp::Socket<'_>) -> R) -> R {
		let mut guard = NIC.lock();
		let nic = guard.as_nic_mut().unwrap();
		f(nic.get_mut_socket::<udp::Socket<'_>>(self.handle))
	}

	async fn close(&self) -> io::Result<()> {
		self.with(|socket| socket.close());
		Ok(())
	}

	async fn write_with_meta(&self, buffer: &[u8], meta: &UdpMetadata) -> io::Result<usize> {
		future::poll_fn(|cx| {
			self.with(|socket| {
				if socket.is_open() {
					if socket.can_send() {
						Poll::Ready(
							socket
								.send_slice(buffer, *meta)
								.map(|()| buffer.len())
								.map_err(|_| Errno::Io),
						)
					} else {
						socket.register_recv_waker(cx.waker());
						Poll::Pending
					}
				} else {
					Poll::Ready(Err(Errno::Io))
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
			self.with(|socket| {
				let ret = if socket.is_open() {
					let mut avail = PollEvent::empty();

					if socket.can_send() {
						avail.insert(
							PollEvent::POLLOUT | PollEvent::POLLWRNORM | PollEvent::POLLWRBAND,
						);
					}

					if socket.can_recv() {
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

	async fn recvfrom(&self, buffer: &mut [MaybeUninit<u8>]) -> io::Result<(usize, Endpoint)> {
		future::poll_fn(|cx| {
			self.with(|socket| {
				if socket.is_open() {
					if socket.can_recv() {
						match socket.recv() {
							// Drop the packet when the provided buffer cannot
							// fit the payload.
							Ok((data, meta)) if data.len() <= buffer.len() => {
								if self.remote_endpoint.is_none_or(|ep| meta.endpoint == ep) {
									buffer[..data.len()].write_copy_of_slice(data);
									Poll::Ready(Ok((data.len(), meta.endpoint)))
								} else {
									socket.register_recv_waker(cx.waker());
									Poll::Pending
								}
							}
							_ => Poll::Ready(Err(Errno::Io)),
						}
					} else {
						socket.register_recv_waker(cx.waker());
						Poll::Pending
					}
				} else {
					Poll::Ready(Err(Errno::Io))
				}
			})
		})
		.await
		.map(|(len, endpoint)| (len, Endpoint::Ip(endpoint)))
	}

	async fn read(&self, buffer: &mut [u8]) -> io::Result<usize> {
		future::poll_fn(|cx| {
			self.with(|socket| {
				if socket.is_open() {
					if socket.can_recv() {
						match socket.recv() {
							// Drop the packet when the provided buffer cannot
							// fit the payload.
							Ok((data, meta)) if data.len() <= buffer.len() => {
								if self.remote_endpoint.is_none_or(|ep| meta.endpoint == ep) {
									buffer[..data.len()].copy_from_slice(data);
									Poll::Ready(Ok(data.len()))
								} else {
									socket.register_recv_waker(cx.waker());
									Poll::Pending
								}
							}
							_ => Poll::Ready(Err(Errno::Io)),
						}
					} else {
						socket.register_recv_waker(cx.waker());
						Poll::Pending
					}
				} else {
					Poll::Ready(Err(Errno::Io))
				}
			})
		})
		.await
	}

	async fn write(&self, buf: &[u8]) -> io::Result<usize> {
		let endpoint = self.remote_endpoint.ok_or(Errno::Inval)?;

		let meta = UdpMetadata::from(endpoint);
		self.write_with_meta(buf, &meta).await
	}

	async fn status_flags(&self) -> io::Result<fd::StatusFlags> {
		let status_flags = if self.nonblocking {
			fd::StatusFlags::O_NONBLOCK
		} else {
			fd::StatusFlags::empty()
		};

		Ok(status_flags)
	}

	async fn set_status_flags(&mut self, status_flags: fd::StatusFlags) -> io::Result<()> {
		self.nonblocking = status_flags.contains(fd::StatusFlags::O_NONBLOCK);
		Ok(())
	}

	async fn getsockname(&self) -> io::Result<Option<Endpoint>> {
		Ok(Some(Endpoint::Ip(self.local_endpoint)))
	}
}

impl Drop for Socket {
	fn drop(&mut self) {
		let _ = block_on(self.close(), None);
		NIC.lock().as_nic_mut().unwrap().destroy_socket(self.handle);
	}
}
