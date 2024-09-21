use alloc::boxed::Box;
use core::future;
use core::ops::DerefMut;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::Poll;

use async_trait::async_trait;
use crossbeam_utils::atomic::AtomicCell;
use smoltcp::socket::udp;
use smoltcp::socket::udp::UdpMetadata;
use smoltcp::wire::IpEndpoint;

use crate::executor::block_on;
use crate::executor::network::{now, Handle, NetworkState, NIC};
use crate::fd::{Endpoint, IoCtl, ListenEndpoint, ObjectInterface, PollEvent};
use crate::io;

#[derive(Debug)]
pub struct Socket {
	handle: Handle,
	nonblocking: AtomicBool,
	endpoint: AtomicCell<Option<IpEndpoint>>,
}

impl Socket {
	pub fn new(handle: Handle) -> Self {
		Self {
			handle,
			nonblocking: AtomicBool::new(false),
			endpoint: AtomicCell::new(None),
		}
	}

	fn with<R>(&self, f: impl FnOnce(&mut udp::Socket<'_>) -> R) -> R {
		let mut guard = NIC.lock();
		let nic = guard.as_nic_mut().unwrap();
		let result = f(nic.get_mut_socket::<udp::Socket<'_>>(self.handle));
		nic.poll_common(now());

		result
	}

	async fn async_close(&self) -> io::Result<()> {
		future::poll_fn(|_cx| {
			self.with(|socket| {
				socket.close();
				Poll::Ready(Ok(()))
			})
		})
		.await
	}

	async fn async_write_with_meta(&self, buffer: &[u8], meta: &UdpMetadata) -> io::Result<usize> {
		future::poll_fn(|cx| {
			self.with(|socket| {
				if socket.is_open() {
					if socket.can_send() {
						Poll::Ready(
							socket
								.send_slice(buffer, *meta)
								.map(|_| buffer.len())
								.map_err(|_| io::Error::EIO),
						)
					} else {
						socket.register_recv_waker(cx.waker());
						Poll::Pending
					}
				} else {
					Poll::Ready(Err(io::Error::EIO))
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

	async fn bind(&self, endpoint: ListenEndpoint) -> io::Result<()> {
		#[allow(irrefutable_let_patterns)]
		if let ListenEndpoint::Ip(endpoint) = endpoint {
			self.with(|socket| socket.bind(endpoint).map_err(|_| io::Error::EADDRINUSE))
		} else {
			Err(io::Error::EIO)
		}
	}

	async fn connect(&self, endpoint: Endpoint) -> io::Result<()> {
		#[allow(irrefutable_let_patterns)]
		if let Endpoint::Ip(endpoint) = endpoint {
			self.endpoint.store(Some(endpoint));
			Ok(())
		} else {
			Err(io::Error::EIO)
		}
	}

	async fn sendto(&self, buf: &[u8], endpoint: Endpoint) -> io::Result<usize> {
		#[allow(irrefutable_let_patterns)]
		if let Endpoint::Ip(endpoint) = endpoint {
			let meta = UdpMetadata::from(endpoint);
			self.async_write_with_meta(buf, &meta).await
		} else {
			Err(io::Error::EIO)
		}
	}

	async fn recvfrom(&self, buffer: &mut [u8]) -> io::Result<(usize, Endpoint)> {
		future::poll_fn(|cx| {
			self.with(|socket| {
				if socket.is_open() {
					if socket.can_recv() {
						match socket.recv_slice(buffer) {
							Ok((len, meta)) => match self.endpoint.load() {
								Some(ep) => {
									if meta.endpoint == ep {
										Poll::Ready(Ok((len, meta.endpoint)))
									} else {
										buffer[..len].iter_mut().for_each(|x| *x = 0);
										socket.register_recv_waker(cx.waker());
										Poll::Pending
									}
								}
								None => Poll::Ready(Ok((len, meta.endpoint))),
							},
							_ => Poll::Ready(Err(io::Error::EIO)),
						}
					} else {
						socket.register_recv_waker(cx.waker());
						Poll::Pending
					}
				} else {
					Poll::Ready(Err(io::Error::EIO))
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
						match socket.recv_slice(buffer) {
							Ok((len, meta)) => match self.endpoint.load() {
								Some(ep) => {
									if meta.endpoint == ep {
										Poll::Ready(Ok(len))
									} else {
										buffer[..len].iter_mut().for_each(|x| *x = 0);
										socket.register_recv_waker(cx.waker());
										Poll::Pending
									}
								}
								None => Poll::Ready(Ok(len)),
							},
							_ => Poll::Ready(Err(io::Error::EIO)),
						}
					} else {
						socket.register_recv_waker(cx.waker());
						Poll::Pending
					}
				} else {
					Poll::Ready(Err(io::Error::EIO))
				}
			})
		})
		.await
	}

	async fn write(&self, buf: &[u8]) -> io::Result<usize> {
		if let Some(endpoint) = self.endpoint.load() {
			let meta = UdpMetadata::from(endpoint);
			self.async_write_with_meta(buf, &meta).await
		} else {
			Err(io::Error::EINVAL)
		}
	}

	async fn ioctl(&self, cmd: IoCtl, value: bool) -> io::Result<()> {
		if cmd == IoCtl::NonBlocking {
			if value {
				info!("set device to nonblocking mode");
				self.nonblocking.store(true, Ordering::Release);
			} else {
				info!("set device to blocking mode");
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
			nic.create_udp_handle().unwrap()
		} else {
			panic!("Unable to create handle");
		};

		Self {
			handle,
			nonblocking: AtomicBool::new(self.nonblocking.load(Ordering::Acquire)),
			endpoint: AtomicCell::new(self.endpoint.load()),
		}
	}
}

impl Drop for Socket {
	fn drop(&mut self) {
		let _ = block_on(self.async_close(), None);
	}
}
