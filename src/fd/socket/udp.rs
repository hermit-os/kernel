use core::ffi::c_void;
use core::future;
use core::marker::PhantomData;
use core::mem::size_of;
use core::ops::DerefMut;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::Poll;

use crossbeam_utils::atomic::AtomicCell;
use smoltcp::socket::udp;
use smoltcp::socket::udp::UdpMetadata;
use smoltcp::time::Duration;
use smoltcp::wire::{IpAddress, IpEndpoint, IpListenEndpoint, Ipv4Address, Ipv6Address};

use crate::executor::network::{block_on, now, poll_on, Handle, NetworkState, NIC};
use crate::fd::{IoError, ObjectInterface};
use crate::syscalls::net::*;

#[derive(Debug)]
pub struct IPv4;

#[derive(Debug)]
pub struct IPv6;

#[derive(Debug)]
pub struct Socket<T> {
	handle: Handle,
	nonblocking: AtomicBool,
	endpoint: AtomicCell<Option<IpEndpoint>>,
	phantom: PhantomData<T>,
}

impl<T> Socket<T> {
	pub fn new(handle: Handle) -> Self {
		Self {
			handle,
			nonblocking: AtomicBool::new(false),
			endpoint: AtomicCell::new(None),
			phantom: PhantomData,
		}
	}

	fn with<R>(&self, f: impl FnOnce(&mut udp::Socket<'_>) -> R) -> R {
		let mut guard = NIC.lock();
		let nic = guard.as_nic_mut().unwrap();
		let result = f(nic.get_mut_socket::<udp::Socket<'_>>(self.handle));
		nic.poll_common(now());

		result
	}

	async fn async_close(&self) -> Result<(), IoError> {
		future::poll_fn(|_cx| {
			self.with(|socket| {
				socket.close();
				Poll::Ready(Ok(()))
			})
		})
		.await
	}

	async fn async_read(&self, buffer: &mut [u8]) -> Result<isize, IoError> {
		future::poll_fn(|cx| {
			self.with(|socket| {
				if socket.is_open() {
					if socket.can_recv() {
						match socket.recv_slice(buffer) {
							Ok((len, meta)) => match self.endpoint.load() {
								Some(ep) => {
									if meta.endpoint == ep {
										Poll::Ready(Ok(len.try_into().unwrap()))
									} else {
										buffer[..len].iter_mut().for_each(|x| *x = 0);
										socket.register_recv_waker(cx.waker());
										Poll::Pending
									}
								}
								None => Poll::Ready(Ok(len.try_into().unwrap())),
							},
							_ => Poll::Ready(Err(IoError::EIO)),
						}
					} else {
						socket.register_recv_waker(cx.waker());
						Poll::Pending
					}
				} else {
					Poll::Ready(Err(IoError::EIO))
				}
			})
		})
		.await
	}

	async fn async_recvfrom(&self, buffer: &mut [u8]) -> Result<(isize, UdpMetadata), IoError> {
		future::poll_fn(|cx| {
			self.with(|socket| {
				if socket.is_open() {
					if socket.can_recv() {
						match socket.recv_slice(buffer) {
							Ok((len, meta)) => match self.endpoint.load() {
								Some(ep) => {
									if meta.endpoint == ep {
										Poll::Ready(Ok((len.try_into().unwrap(), meta)))
									} else {
										buffer[..len].iter_mut().for_each(|x| *x = 0);
										socket.register_recv_waker(cx.waker());
										Poll::Pending
									}
								}
								None => Poll::Ready(Ok((len.try_into().unwrap(), meta))),
							},
							_ => Poll::Ready(Err(IoError::EIO)),
						}
					} else {
						socket.register_recv_waker(cx.waker());
						Poll::Pending
					}
				} else {
					Poll::Ready(Err(IoError::EIO))
				}
			})
		})
		.await
	}

	async fn async_write(&self, buffer: &[u8], meta: &UdpMetadata) -> Result<isize, IoError> {
		future::poll_fn(|cx| {
			self.with(|socket| {
				if socket.is_open() {
					if socket.can_send() {
						Poll::Ready(
							socket
								.send_slice(buffer, *meta)
								.map(|_| buffer.len() as isize)
								.map_err(|_| IoError::EIO),
						)
					} else {
						socket.register_recv_waker(cx.waker());
						Poll::Pending
					}
				} else {
					Poll::Ready(Err(IoError::EIO))
				}
			})
		})
		.await
	}

	fn read(&self, buf: &mut [u8]) -> Result<isize, IoError> {
		if buf.len() == 0 {
			return Ok(0);
		}

		if self.nonblocking.load(Ordering::Acquire) {
			poll_on(self.async_read(buf), Some(Duration::ZERO)).map_err(|x| {
				if x == IoError::ETIME {
					IoError::EAGAIN
				} else {
					x
				}
			})
		} else {
			match poll_on(self.async_read(buf), Some(Duration::from_secs(2))) {
				Err(IoError::ETIME) => block_on(self.async_read(buf), None),
				Err(x) => Err(x),
				Ok(x) => Ok(x),
			}
		}
	}

	fn write(&self, buf: &[u8]) -> Result<isize, IoError> {
		if buf.len() == 0 {
			return Ok(0);
		}

		let endpoint = self.endpoint.load();
		if endpoint.is_none() {
			return Err(IoError::EINVAL);
		}

		let meta = UdpMetadata::from(endpoint.unwrap());

		if self.nonblocking.load(Ordering::Acquire) {
			poll_on(self.async_write(buf, &meta), Some(Duration::ZERO))
		} else {
			poll_on(self.async_write(buf, &meta), None)
		}
	}

	fn ioctl(&self, cmd: i32, argp: *mut c_void) -> Result<(), IoError> {
		if cmd == FIONBIO {
			let value = unsafe { *(argp as *const i32) };
			if value != 0 {
				info!("set device to nonblocking mode");
				self.nonblocking.store(true, Ordering::Release);
			} else {
				info!("set device to blocking mode");
				self.nonblocking.store(false, Ordering::Release);
			}

			Ok(())
		} else {
			Err(IoError::EINVAL)
		}
	}
}

impl<T> Clone for Socket<T> {
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
			phantom: PhantomData,
		}
	}
}

impl<T> Drop for Socket<T> {
	fn drop(&mut self) {
		let _ = block_on(self.async_close(), None);
	}
}

impl ObjectInterface for Socket<IPv4> {
	fn bind(&self, name: *const sockaddr, namelen: socklen_t) -> Result<(), IoError> {
		if namelen == size_of::<sockaddr_in>().try_into().unwrap() {
			let addr = unsafe { *(name as *const sockaddr_in) };
			let s_addr = addr.sin_addr.s_addr;
			let port = u16::from_be(addr.sin_port);
			let endpoint = if s_addr.into_iter().all(|b| b == 0) {
				IpListenEndpoint { addr: None, port }
			} else {
				IpListenEndpoint {
					addr: Some(IpAddress::v4(s_addr[0], s_addr[1], s_addr[2], s_addr[3])),
					port,
				}
			};
			self.with(|socket| {
				if !socket.is_open() {
					let _ = socket.bind(endpoint).unwrap();
					debug!("{:?}", endpoint);
				}
			});

			Ok(())
		} else {
			Err(IoError::EINVAL)
		}
	}

	fn connect(&self, name: *const sockaddr, namelen: socklen_t) -> Result<i32, IoError> {
		if namelen == size_of::<sockaddr_in>().try_into().unwrap() {
			let saddr = unsafe { *(name as *const sockaddr_in) };
			let port = u16::from_be(saddr.sin_port);
			let address = IpAddress::v4(
				saddr.sin_addr.s_addr[0],
				saddr.sin_addr.s_addr[1],
				saddr.sin_addr.s_addr[2],
				saddr.sin_addr.s_addr[3],
			);

			self.endpoint.store(Some(IpEndpoint::new(address, port)));

			Ok(0)
		} else {
			Err(IoError::EINVAL)
		}
	}

	fn read(&self, buf: &mut [u8]) -> Result<isize, IoError> {
		self.read(buf)
	}

	fn write(&self, buf: &[u8]) -> Result<isize, IoError> {
		self.write(buf)
	}

	fn sendto(
		&self,
		buf: &[u8],
		addr: *const sockaddr,
		addr_len: socklen_t,
	) -> Result<isize, IoError> {
		if addr.is_null() || addr_len == 0 {
			self.write(buf)
		} else {
			if addr_len >= size_of::<sockaddr_in>().try_into().unwrap() {
				let addr = unsafe { &*(addr as *const sockaddr_in) };
				let ip = IpAddress::from(Ipv4Address::from_bytes(&addr.sin_addr.s_addr[0..]));
				let endpoint = IpEndpoint::new(ip, u16::from_be(addr.sin_port));
				self.endpoint.store(Some(endpoint));
				let meta = UdpMetadata::from(endpoint);

				if self.nonblocking.load(Ordering::Acquire) {
					poll_on(self.async_write(buf, &meta), Some(Duration::ZERO)).map_err(|x| {
						if x == IoError::ETIME {
							IoError::EAGAIN
						} else {
							x
						}
					})
				} else {
					poll_on(self.async_write(buf, &meta), None)
				}
			} else {
				Err(IoError::EINVAL)
			}
		}
	}

	fn recvfrom(
		&self,
		buf: &mut [u8],
		address: *mut sockaddr,
		address_len: *mut socklen_t,
	) -> Result<isize, IoError> {
		if !address_len.is_null() {
			let len = unsafe { &mut *address_len };
			if *len < size_of::<sockaddr_in>().try_into().unwrap() {
				return Err(IoError::EINVAL);
			}
		}

		if buf.len() == 0 {
			return Err(IoError::EINVAL);
		}

		if self.nonblocking.load(Ordering::Acquire) {
			match poll_on(self.async_recvfrom(buf), Some(Duration::ZERO)) {
				Err(IoError::ETIME) => Err(IoError::EAGAIN),
				Err(e) => Err(e),
				Ok((x, meta)) => {
					let len = unsafe { &mut *address_len };
					if address.is_null() {
						*len = 0;
					} else {
						let addr = unsafe { &mut *(address as *mut sockaddr_in) };
						addr.sin_port = meta.endpoint.port.to_be();
						if let IpAddress::Ipv4(ip) = meta.endpoint.addr {
							addr.sin_addr.s_addr.copy_from_slice(ip.as_bytes());
						}
						*len = size_of::<sockaddr_in>().try_into().unwrap();
					}
					Ok(x)
				}
			}
		} else {
			match poll_on(self.async_recvfrom(buf), Some(Duration::from_secs(2))) {
				Err(IoError::ETIME) => block_on(self.async_recvfrom(buf), None).map(|(x, meta)| {
					let len = unsafe { &mut *address_len };
					if address.is_null() {
						*len = 0;
					} else {
						let addr = unsafe { &mut *(address as *mut sockaddr_in) };
						addr.sin_port = meta.endpoint.port.to_be();
						if let IpAddress::Ipv4(ip) = meta.endpoint.addr {
							addr.sin_addr.s_addr.copy_from_slice(ip.as_bytes());
						}
						*len = size_of::<sockaddr_in>().try_into().unwrap();
					}
					x
				}),
				Err(e) => Err(e),
				Ok((x, meta)) => {
					let len = unsafe { &mut *address_len };
					if address.is_null() {
						*len = 0;
					} else {
						let addr = unsafe { &mut *(address as *mut sockaddr_in) };
						addr.sin_port = meta.endpoint.port.to_be();
						if let IpAddress::Ipv4(ip) = meta.endpoint.addr {
							addr.sin_addr.s_addr.copy_from_slice(ip.as_bytes());
						}
						*len = size_of::<sockaddr_in>().try_into().unwrap();
					}
					Ok(x)
				}
			}
		}
	}
}

impl ObjectInterface for Socket<IPv6> {
	fn bind(&self, name: *const sockaddr, namelen: socklen_t) -> Result<(), IoError> {
		if namelen == size_of::<sockaddr_in6>().try_into().unwrap() {
			let addr = unsafe { *(name as *const sockaddr_in6) };
			let s6_addr = addr.sin6_addr.s6_addr;
			let port = u16::from_be(addr.sin6_port);
			let endpoint = if s6_addr.into_iter().all(|b| b == 0) {
				IpListenEndpoint { addr: None, port }
			} else {
				let addr = IpAddress::v6(
					u16::from_ne_bytes(s6_addr[0..1].try_into().unwrap()),
					u16::from_ne_bytes(s6_addr[2..3].try_into().unwrap()),
					u16::from_ne_bytes(s6_addr[4..5].try_into().unwrap()),
					u16::from_ne_bytes(s6_addr[6..7].try_into().unwrap()),
					u16::from_ne_bytes(s6_addr[8..9].try_into().unwrap()),
					u16::from_ne_bytes(s6_addr[10..11].try_into().unwrap()),
					u16::from_ne_bytes(s6_addr[12..13].try_into().unwrap()),
					u16::from_ne_bytes(s6_addr[14..15].try_into().unwrap()),
				);

				IpListenEndpoint {
					addr: Some(addr),
					port,
				}
			};
			self.with(|socket| {
				if !socket.is_open() {
					let _ = socket.bind(endpoint).unwrap();
				}
			});

			Ok(())
		} else {
			Err(IoError::EINVAL)
		}
	}

	fn connect(&self, name: *const sockaddr, namelen: socklen_t) -> Result<i32, IoError> {
		if namelen == size_of::<sockaddr_in6>().try_into().unwrap() {
			let saddr = unsafe { *(name as *const sockaddr_in6) };
			let s6_addr = saddr.sin6_addr.s6_addr;
			let port = u16::from_be(saddr.sin6_port);
			let address = IpAddress::v6(
				u16::from_ne_bytes(s6_addr[0..1].try_into().unwrap()),
				u16::from_ne_bytes(s6_addr[2..3].try_into().unwrap()),
				u16::from_ne_bytes(s6_addr[4..5].try_into().unwrap()),
				u16::from_ne_bytes(s6_addr[6..7].try_into().unwrap()),
				u16::from_ne_bytes(s6_addr[8..9].try_into().unwrap()),
				u16::from_ne_bytes(s6_addr[10..11].try_into().unwrap()),
				u16::from_ne_bytes(s6_addr[12..13].try_into().unwrap()),
				u16::from_ne_bytes(s6_addr[14..15].try_into().unwrap()),
			);

			self.endpoint.store(Some(IpEndpoint::new(address, port)));

			Ok(0)
		} else {
			Err(IoError::EINVAL)
		}
	}

	fn read(&self, buf: &mut [u8]) -> Result<isize, IoError> {
		self.read(buf)
	}

	fn sendto(
		&self,
		buf: &[u8],
		addr: *const sockaddr,
		addr_len: socklen_t,
	) -> Result<isize, IoError> {
		if addr.is_null() || addr_len == 0 {
			self.write(buf)
		} else {
			if addr_len >= size_of::<sockaddr_in6>().try_into().unwrap() {
				let addr = unsafe { &*(addr as *const sockaddr_in6) };
				let ip = IpAddress::from(Ipv6Address::from_bytes(&addr.sin6_addr.s6_addr[0..]));
				let endpoint = IpEndpoint::new(ip, u16::from_be(addr.sin6_port));
				self.endpoint.store(Some(endpoint));
				let meta = UdpMetadata::from(endpoint);

				if self.nonblocking.load(Ordering::Acquire) {
					poll_on(self.async_write(buf, &meta), Some(Duration::ZERO)).map_err(|x| {
						if x == IoError::ETIME {
							IoError::EAGAIN
						} else {
							x
						}
					})
				} else {
					poll_on(self.async_write(buf, &meta), None)
				}
			} else {
				Err(IoError::EINVAL)
			}
		}
	}

	fn recvfrom(
		&self,
		buf: &mut [u8],
		address: *mut sockaddr,
		address_len: *mut socklen_t,
	) -> Result<isize, IoError> {
		if !address_len.is_null() {
			let len = unsafe { &mut *address_len };
			if *len < size_of::<sockaddr_in6>().try_into().unwrap() {
				return Err(IoError::EINVAL);
			}
		}

		if buf.len() == 0 {
			return Err(IoError::EINVAL);
		}

		if self.nonblocking.load(Ordering::Acquire) {
			poll_on(self.async_recvfrom(buf), Some(Duration::ZERO))
				.map_err(|x| {
					if x == IoError::ETIME {
						IoError::EAGAIN
					} else {
						x
					}
				})
				.map(|(x, meta)| {
					let len = unsafe { &mut *address_len };
					if address.is_null() {
						*len = 0;
					} else {
						let addr = unsafe { &mut *(address as *mut sockaddr_in6) };
						addr.sin6_port = meta.endpoint.port.to_be();
						if let IpAddress::Ipv6(ip) = meta.endpoint.addr {
							addr.sin6_addr.s6_addr.copy_from_slice(ip.as_bytes());
						}
						*len = size_of::<sockaddr_in6>().try_into().unwrap();
					}
					x
				})
		} else {
			match poll_on(self.async_recvfrom(buf), Some(Duration::from_secs(2))) {
				Err(IoError::ETIME) => block_on(self.async_recvfrom(buf), None).map(|(x, meta)| {
					let len = unsafe { &mut *address_len };
					if address.is_null() {
						*len = 0;
					} else {
						let addr = unsafe { &mut *(address as *mut sockaddr_in6) };
						addr.sin6_port = meta.endpoint.port.to_be();
						if let IpAddress::Ipv6(ip) = meta.endpoint.addr {
							addr.sin6_addr.s6_addr.copy_from_slice(ip.as_bytes());
						}
						*len = size_of::<sockaddr_in6>().try_into().unwrap();
					}
					x
				}),
				Err(e) => Err(e),
				Ok((x, meta)) => {
					let len = unsafe { &mut *address_len };
					if address.is_null() {
						*len = 0;
					} else {
						let addr = unsafe { &mut *(address as *mut sockaddr_in6) };
						addr.sin6_port = meta.endpoint.port.to_be();
						if let IpAddress::Ipv6(ip) = meta.endpoint.addr {
							addr.sin6_addr.s6_addr.copy_from_slice(ip.as_bytes());
						}
						*len = size_of::<sockaddr_in6>().try_into().unwrap();
					}
					Ok(x)
				}
			}
		}
	}

	fn write(&self, buf: &[u8]) -> Result<isize, IoError> {
		self.write(buf)
	}

	fn ioctl(&self, cmd: i32, argp: *mut c_void) -> Result<(), IoError> {
		self.ioctl(cmd, argp)
	}
}
