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
use smoltcp::wire::{IpAddress, IpEndpoint, IpListenEndpoint, Ipv4Address};

use crate::errno::*;
use crate::executor::network::{block_on, now, poll_on, Handle, NetworkState, NIC};
use crate::fd::ObjectInterface;
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

	async fn async_close(&self) -> Result<(), i32> {
		future::poll_fn(|_cx| {
			self.with(|socket| {
				socket.close();
				Poll::Ready(Ok(()))
			})
		})
		.await
	}

	async fn async_read(&self, buffer: &mut [u8]) -> Result<isize, i32> {
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
							_ => Poll::Ready(Err(-crate::errno::EIO)),
						}
					} else {
						socket.register_recv_waker(cx.waker());
						Poll::Pending
					}
				} else {
					Poll::Ready(Err(-crate::errno::EIO))
				}
			})
		})
		.await
	}

	async fn async_recvfrom(&self, buffer: &mut [u8]) -> Result<(isize, UdpMetadata), i32> {
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
							_ => Poll::Ready(Err(-crate::errno::EIO)),
						}
					} else {
						socket.register_recv_waker(cx.waker());
						Poll::Pending
					}
				} else {
					Poll::Ready(Err(-crate::errno::EIO))
				}
			})
		})
		.await
	}

	async fn async_write(&self, buffer: &[u8], meta: UdpMetadata) -> Result<isize, i32> {
		future::poll_fn(|cx| {
			self.with(|socket| {
				if socket.is_open() {
					if socket.can_send() {
						Poll::Ready(
							socket
								.send_slice(buffer, meta)
								.map(|_| buffer.len() as isize)
								.map_err(|_| -crate::errno::EIO),
						)
					} else {
						socket.register_recv_waker(cx.waker());
						Poll::Pending
					}
				} else {
					Poll::Ready(Err(-crate::errno::EIO))
				}
			})
		})
		.await
	}

	fn read(&self, buf: *mut u8, len: usize) -> isize {
		if len == 0 {
			return 0;
		}

		let slice = unsafe { core::slice::from_raw_parts_mut(buf, len) };

		if self.nonblocking.load(Ordering::Acquire) {
			poll_on(self.async_read(slice), Some(Duration::ZERO)).unwrap_or_else(|x| {
				if x == -ETIME {
					(-EAGAIN).try_into().unwrap()
				} else {
					x.try_into().unwrap()
				}
			})
		} else {
			poll_on(self.async_read(slice), Some(Duration::from_secs(2))).unwrap_or_else(|x| {
				if x == -ETIME {
					block_on(self.async_read(slice), None).unwrap_or_else(|y| y.try_into().unwrap())
				} else {
					x.try_into().unwrap()
				}
			})
		}
	}

	fn write(&self, buf: *const u8, len: usize) -> isize {
		if len == 0 {
			return 0;
		}

		let endpoint = self.endpoint.load();
		if endpoint.is_none() {
			return (-EINVAL).try_into().unwrap();
		}

		let meta = UdpMetadata::from(endpoint.unwrap());
		let slice = unsafe { core::slice::from_raw_parts(buf, len) };

		if self.nonblocking.load(Ordering::Acquire) {
			poll_on(self.async_write(slice, meta), Some(Duration::ZERO)).unwrap_or_else(|x| {
				if x == -ETIME {
					(-EAGAIN).try_into().unwrap()
				} else {
					x.try_into().unwrap()
				}
			})
		} else {
			poll_on(self.async_write(slice, meta), None).unwrap_or_else(|x| x.try_into().unwrap())
		}
	}

	fn ioctl(&self, cmd: i32, argp: *mut c_void) -> i32 {
		if cmd == FIONBIO {
			let value = unsafe { *(argp as *const i32) };
			if value != 0 {
				info!("set device to nonblocking mode");
				self.nonblocking.store(true, Ordering::Release);
			} else {
				info!("set device to blocking mode");
				self.nonblocking.store(false, Ordering::Release);
			}

			0
		} else {
			-EINVAL
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
	fn bind(&self, name: *const sockaddr, namelen: socklen_t) -> i32 {
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
					info!("{:?}", endpoint);
				}
			});

			0
		} else {
			-EINVAL
		}
	}

	fn connect(&self, name: *const sockaddr, namelen: socklen_t) -> i32 {
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

			0
		} else {
			-EINVAL
		}
	}

	fn read(&self, buf: *mut u8, len: usize) -> isize {
		self.read(buf, len)
	}

	fn write(&self, buf: *const u8, len: usize) -> isize {
		self.write(buf, len)
	}

	fn sendto(
		&self,
		buf: *const u8,
		len: usize,
		addr: *const sockaddr,
		addr_len: socklen_t,
	) -> isize {
		if addr.is_null() || addr_len == 0 {
			self.write(buf, len)
		} else {
			if addr_len >= size_of::<sockaddr_in>().try_into().unwrap() {
				let addr = unsafe { &*(addr as *const sockaddr_in) };
				let ip = IpAddress::from(Ipv4Address::from_bytes(&addr.sin_addr.s_addr[0..]));
				let endpoint = IpEndpoint::new(ip, u16::from_be(addr.sin_port));
				self.endpoint.store(Some(endpoint));
				let meta = UdpMetadata::from(endpoint);
				let slice = unsafe { core::slice::from_raw_parts(buf, len) };

				if self.nonblocking.load(Ordering::Acquire) {
					poll_on(self.async_write(slice, meta), Some(Duration::ZERO)).unwrap_or_else(
						|x| {
							if x == -ETIME {
								(-EAGAIN).try_into().unwrap()
							} else {
								x.try_into().unwrap()
							}
						},
					)
				} else {
					poll_on(self.async_write(slice, meta), None)
						.unwrap_or_else(|x| x.try_into().unwrap())
				}
			} else {
				(-EINVAL).try_into().unwrap()
			}
		}
	}

	fn recvfrom(
		&self,
		buf: *mut u8,
		len: usize,
		address: *mut sockaddr,
		address_len: *mut socklen_t,
	) -> isize {
		if !address_len.is_null() {
			let len = unsafe { &mut *address_len };
			if *len < size_of::<sockaddr_in>().try_into().unwrap() {
				return (-EINVAL).try_into().unwrap();
			}
		}

		if len == 0 {
			return (-EINVAL).try_into().unwrap();
		}

		let slice = unsafe { core::slice::from_raw_parts_mut(buf, len) };

		if self.nonblocking.load(Ordering::Acquire) {
			poll_on(self.async_recvfrom(slice), Some(Duration::ZERO)).map_or_else(
				|x| {
					if x == -ETIME {
						(-EAGAIN).try_into().unwrap()
					} else {
						x.try_into().unwrap()
					}
				},
				|(x, meta)| {
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
					x.try_into().unwrap()
				},
			)
		} else {
			poll_on(self.async_recvfrom(slice), Some(Duration::from_secs(2))).map_or_else(
				|x| {
					if x == -ETIME {
						block_on(self.async_recvfrom(slice), None).map_or_else(
							|x| x.try_into().unwrap(),
							|(x, meta)| {
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
								x.try_into().unwrap()
							},
						)
					} else {
						x.try_into().unwrap()
					}
				},
				|(x, meta)| {
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
					x.try_into().unwrap()
				},
			)
		}
	}
}

impl ObjectInterface for Socket<IPv6> {
	fn bind(&self, name: *const sockaddr, namelen: socklen_t) -> i32 {
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

			0
		} else {
			-EINVAL
		}
	}

	fn connect(&self, name: *const sockaddr, namelen: socklen_t) -> i32 {
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

			0
		} else {
			-EINVAL
		}
	}

	fn read(&self, buf: *mut u8, len: usize) -> isize {
		self.read(buf, len)
	}

	fn sendto(
		&self,
		buf: *const u8,
		len: usize,
		addr: *const sockaddr,
		addr_len: socklen_t,
	) -> isize {
		if addr.is_null() || addr_len == 0 {
			self.write(buf, len)
		} else {
			if addr_len >= size_of::<sockaddr_in6>().try_into().unwrap() {
				let addr = unsafe { &*(addr as *const sockaddr_in6) };
				let ip = IpAddress::from(Ipv4Address::from_bytes(&addr.sin6_addr.s6_addr[0..]));
				let endpoint = IpEndpoint::new(ip, u16::from_be(addr.sin6_port));
				self.endpoint.store(Some(endpoint));
				let meta = UdpMetadata::from(endpoint);
				let slice = unsafe { core::slice::from_raw_parts(buf, len) };

				if self.nonblocking.load(Ordering::Acquire) {
					poll_on(self.async_write(slice, meta), Some(Duration::ZERO)).unwrap_or_else(
						|x| {
							if x == -ETIME {
								(-EAGAIN).try_into().unwrap()
							} else {
								x.try_into().unwrap()
							}
						},
					)
				} else {
					poll_on(self.async_write(slice, meta), None)
						.unwrap_or_else(|x| x.try_into().unwrap())
				}
			} else {
				(-EINVAL).try_into().unwrap()
			}
		}
	}

	fn recvfrom(
		&self,
		buf: *mut u8,
		len: usize,
		address: *mut sockaddr,
		address_len: *mut socklen_t,
	) -> isize {
		if !address_len.is_null() {
			let len = unsafe { &mut *address_len };
			if *len < size_of::<sockaddr_in6>().try_into().unwrap() {
				return (-EINVAL).try_into().unwrap();
			}
		}

		if len == 0 {
			return (-EINVAL).try_into().unwrap();
		}

		let slice = unsafe { core::slice::from_raw_parts_mut(buf, len) };

		if self.nonblocking.load(Ordering::Acquire) {
			poll_on(self.async_recvfrom(slice), Some(Duration::ZERO)).map_or_else(
				|x| {
					if x == -ETIME {
						(-EAGAIN).try_into().unwrap()
					} else {
						x.try_into().unwrap()
					}
				},
				|(x, meta)| {
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
					x.try_into().unwrap()
				},
			)
		} else {
			poll_on(self.async_recvfrom(slice), Some(Duration::from_secs(2))).map_or_else(
				|x| {
					if x == -ETIME {
						block_on(self.async_recvfrom(slice), None).map_or_else(
							|x| x.try_into().unwrap(),
							|(x, meta)| {
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
								x.try_into().unwrap()
							},
						)
					} else {
						x.try_into().unwrap()
					}
				},
				|(x, meta)| {
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
					x.try_into().unwrap()
				},
			)
		}
	}

	fn write(&self, buf: *const u8, len: usize) -> isize {
		self.write(buf, len)
	}

	fn ioctl(&self, cmd: i32, argp: *mut c_void) -> i32 {
		self.ioctl(cmd, argp)
	}
}
