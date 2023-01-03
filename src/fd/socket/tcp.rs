use core::ffi::c_void;
use core::marker::PhantomData;
use core::mem::size_of;
use core::ops::DerefMut;
use core::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use core::task::Poll;

use futures_lite::future;
use smoltcp::iface;
use smoltcp::socket::{TcpSocket, TcpState};
use smoltcp::time::Duration;
use smoltcp::wire::IpAddress;

use crate::errno::*;
use crate::fd::ObjectInterface;
use crate::net::executor::block_on;
use crate::net::{now, Handle, NetworkState, NIC};
use crate::syscalls::net::*;
use crate::DEFAULT_KEEP_ALIVE_INTERVAL;

fn get_ephemeral_port() -> u16 {
	static LOCAL_ENDPOINT: AtomicU16 = AtomicU16::new(49152);

	LOCAL_ENDPOINT.fetch_add(1, Ordering::SeqCst)
}

#[derive(Debug)]
pub struct IPv4;

#[derive(Debug)]
pub struct IPv6;

#[derive(Debug)]
pub struct Socket<T> {
	handle: Handle,
	port: AtomicU16,
	nonblocking: AtomicBool,
	phantom: PhantomData<T>,
}

impl<T> Socket<T> {
	pub fn new(handle: Handle) -> Self {
		Self {
			handle,
			port: AtomicU16::new(0),
			nonblocking: AtomicBool::new(false),
			phantom: PhantomData,
		}
	}

	fn with<R>(&self, f: impl FnOnce(&mut TcpSocket<'_>) -> R) -> R {
		let mut guard = NIC.lock();
		let nic = guard.as_nic_mut().unwrap();
		let res = {
			let s = nic.iface.get_socket::<TcpSocket<'_>>(self.handle);
			f(s)
		};
		let t = now();
		if nic.poll_delay(t).map(|d| d.total_millis()).unwrap_or(0) == 0 {
			nic.poll_common(t);
		}
		res
	}

	fn with_context<R>(
		&self,
		f: impl FnOnce(&mut TcpSocket<'_>, &mut iface::Context<'_>) -> R,
	) -> R {
		let mut guard = NIC.lock();
		let nic = guard.as_nic_mut().unwrap();
		let res = {
			let (s, cx) = nic
				.iface
				.get_socket_and_context::<TcpSocket<'_>>(self.handle);
			f(s, cx)
		};
		let t = now();
		if nic.poll_delay(t).map(|d| d.total_millis()).unwrap_or(0) == 0 {
			nic.poll_common(t);
		}
		res
	}

	async fn async_read(&self, buffer: &mut [u8]) -> Result<isize, i32> {
		future::poll_fn(|cx| {
			self.with(|socket| {
				if socket.can_recv() {
					let n = socket.recv_slice(buffer).map_err(|_| -crate::errno::EIO)?;
					if n > 0 || buffer.is_empty() {
						return Poll::Ready(Ok(n.try_into().unwrap()));
					}
				}

				match socket.state() {
					TcpState::FinWait1
					| TcpState::FinWait2
					| TcpState::Closed
					| TcpState::Closing
					| TcpState::CloseWait
					| TcpState::TimeWait => Poll::Ready(Err(-crate::errno::EIO)),
					_ => {
						socket.register_recv_waker(cx.waker());
						Poll::Pending
					}
				}
			})
		})
		.await
	}

	async fn async_connect(&self, address: IpAddress, port: u16) -> Result<i32, i32> {
		self.with_context(|socket, cx| socket.connect(cx, (address, port), get_ephemeral_port()))
			.map_err(|x| {
				info!("x {}", x);
				-crate::errno::EIO
			})?;

		future::poll_fn(|cx| {
			self.with(|socket| match socket.state() {
				TcpState::Closed | TcpState::TimeWait => Poll::Ready(Err(-crate::errno::EFAULT)),
				TcpState::Listen => Poll::Ready(Err(-crate::errno::EIO)),
				TcpState::SynSent | TcpState::SynReceived => {
					socket.register_send_waker(cx.waker());
					Poll::Pending
				}
				_ => Poll::Ready(Ok(0)),
			})
		})
		.await
	}

	async fn async_write(&self, buffer: &[u8]) -> Result<isize, i32> {
		let len = buffer.len();
		let mut pos: usize = 0;

		while pos < len {
			let n = future::poll_fn(|cx| {
				self.with(|socket| match socket.state() {
					TcpState::FinWait1
					| TcpState::FinWait2
					| TcpState::Closed
					| TcpState::Closing
					| TcpState::CloseWait
					| TcpState::TimeWait => Poll::Ready(Err(-crate::errno::EIO)),
					_ => {
						if !socket.may_send() {
							return Poll::Ready(Err(-crate::errno::EIO));
						} else if socket.can_send() {
							return Poll::Ready(
								socket
									.send_slice(&buffer[pos..])
									.map_err(|_| -crate::errno::EIO),
							);
						}

						if pos > 0 {
							// we already send some data => return 0 as signal to stop the
							// async write
							return Poll::Ready(Ok(0));
						}

						socket.register_send_waker(cx.waker());
						Poll::Pending
					}
				})
			})
			.await?;

			if n == 0 {
				return Ok(pos.try_into().unwrap());
			}

			pos += n;
		}

		Ok(pos.try_into().unwrap())
	}

	async fn async_close(&self) -> Result<(), i32> {
		future::poll_fn(|cx| {
			self.with(|socket| match socket.state() {
				TcpState::FinWait1
				| TcpState::FinWait2
				| TcpState::Closed
				| TcpState::Closing
				| TcpState::TimeWait => Poll::Ready(Err(-crate::errno::EIO)),
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
				TcpState::FinWait1
				| TcpState::FinWait2
				| TcpState::Closed
				| TcpState::Closing
				| TcpState::TimeWait => Poll::Ready(Ok(())),
				_ => {
					socket.register_send_waker(cx.waker());
					Poll::Pending
				}
			})
		})
		.await
	}

	async fn async_accept(
		&self,
		_addr: *mut sockaddr,
		_addrlen: *mut socklen_t,
	) -> Result<(), i32> {
		future::poll_fn(|cx| {
			self.with(|socket| match socket.state() {
				TcpState::Closed => {
					let _ = socket.listen(self.port.load(Ordering::Acquire));
					Poll::Ready(())
				}
				TcpState::Listen => Poll::Ready(()),
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
						TcpState::Closed
						| TcpState::Closing
						| TcpState::FinWait1
						| TcpState::FinWait2 => Poll::Ready(Err(-crate::errno::EIO)),
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
		let nic = guard.as_nic_mut().map_err(|_| -crate::errno::EIO)?;
		let socket = nic.iface.get_socket::<TcpSocket<'_>>(self.handle);
		socket.set_keep_alive(Some(Duration::from_millis(DEFAULT_KEEP_ALIVE_INTERVAL)));

		Ok(())
	}
}

impl<T: core::marker::Sync + core::marker::Send + core::fmt::Debug + 'static> ObjectInterface
	for Socket<T>
{
	default fn bind(&self, _name: *const sockaddr, _namelen: socklen_t) -> i32 {
		-EINVAL
	}

	default fn getsockname(&self, _name: *mut sockaddr, _namelen: *mut socklen_t) -> i32 {
		-EINVAL
	}

	default fn getpeername(&self, _name: *mut sockaddr, _namelen: *mut socklen_t) -> i32 {
		-EINVAL
	}

	default fn connect(&self, _name: *const sockaddr, _namelen: socklen_t) -> i32 {
		-EINVAL
	}

	fn accept(&self, addr: *mut sockaddr, addrlen: *mut socklen_t) -> i32 {
		block_on(self.async_accept(addr, addrlen), None)
			.map(|_| 0)
			.unwrap_or_else(|x| x)
	}

	fn read(&self, buf: *mut u8, len: usize) -> isize {
		let slice = unsafe { core::slice::from_raw_parts_mut(buf, len) };

		if self.nonblocking.load(Ordering::Acquire) {
			block_on(self.async_read(slice), Some(Duration::ZERO)).unwrap_or_else(|x| {
				if x == -ETIME {
					(-EAGAIN).try_into().unwrap()
				} else {
					x.try_into().unwrap()
				}
			})
		} else {
			block_on(self.async_read(slice), None).unwrap_or_else(|x| x.try_into().unwrap())
		}
	}

	fn write(&self, buf: *const u8, len: usize) -> isize {
		let slice = unsafe { core::slice::from_raw_parts(buf, len) };

		if self.nonblocking.load(Ordering::Acquire) {
			block_on(self.async_write(slice), Some(Duration::ZERO)).unwrap_or_else(|x| {
				if x == -ETIME {
					(-EAGAIN).try_into().unwrap()
				} else {
					x.try_into().unwrap()
				}
			})
		} else {
			block_on(self.async_write(slice), None).unwrap_or_else(|x| x.try_into().unwrap())
		}
	}

	fn listen(&self, _backlog: i32) -> i32 {
		self.with(|socket| {
			if !socket.is_open() {
				socket
					.listen(self.port.load(Ordering::Acquire))
					.map(|_| 0)
					.unwrap_or_else(|_| -crate::errno::EIO)
			} else {
				-crate::errno::EIO
			}
		})
	}

	fn setsockopt(
		&self,
		level: i32,
		optname: i32,
		optval: *const c_void,
		optlen: socklen_t,
	) -> i32 {
		if level == IPPROTO_TCP
			&& optname == TCP_NODELAY
			&& optlen == size_of::<i32>().try_into().unwrap()
		{
			let value = unsafe { *(optval as *const i32) };
			self.with(|socket| socket.set_nagle_enabled(value != 0));
			0
		} else if level == SOL_SOCKET && optname == SO_REUSEADDR {
			// smoltcp is always able to reuse the addr
			0
		} else {
			-EINVAL
		}
	}

	fn getsockopt(
		&self,
		level: i32,
		optname: i32,
		optval: *mut c_void,
		optlen: *mut socklen_t,
	) -> i32 {
		if level == IPPROTO_TCP && optname == TCP_NODELAY {
			let optlen = unsafe { &mut *optlen };
			if *optlen >= size_of::<i32>().try_into().unwrap() {
				let optval = unsafe { &mut *(optval as *mut i32) };
				self.with(|socket| {
					if socket.nagle_enabled() {
						*optval = 0;
					} else {
						*optval = 1;
					}
				});
				*optlen = size_of::<i32>().try_into().unwrap();

				0
			} else {
				-EINVAL
			}
		} else {
			-EINVAL
		}
	}

	fn shutdown(&self, how: i32) -> i32 {
		match how {
			SHUT_RD /* Read  */ |
			SHUT_WR /* Write */ |
			SHUT_RDWR /* Both */ => 0,
			_ => -EINVAL,
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
			nic.create_tcp_handle().unwrap()
		} else {
			panic!("Unable to create handle");
		};

		Self {
			handle,
			port: AtomicU16::new(self.port.load(Ordering::Acquire)),
			nonblocking: AtomicBool::new(self.nonblocking.load(Ordering::Acquire)),
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
			let port = u16::from_be(addr.sin_port);
			self.port.store(port, Ordering::Release);
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

			if self.nonblocking.load(Ordering::Acquire) {
				block_on(self.async_connect(address, port), Some(Duration::ZERO)).unwrap_or_else(
					|x| {
						if x == -ETIME {
							-EAGAIN
						} else {
							x
						}
					},
				)
			} else {
				block_on(self.async_connect(address, port), None).unwrap_or_else(|x| x)
			}
		} else {
			-EINVAL
		}
	}

	fn getpeername(&self, name: *mut sockaddr, namelen: *mut socklen_t) -> i32 {
		if namelen.is_null() {
			return -ENOBUFS;
		}

		let namelen = unsafe { &mut *namelen };
		if *namelen >= size_of::<sockaddr_in>().try_into().unwrap() {
			let addr = unsafe { &mut *(name as *mut sockaddr_in) };

			self.with(|socket| {
				let remote = socket.remote_endpoint();
				addr.sin_port = remote.port.to_be();

				if let IpAddress::Ipv4(ip) = remote.addr {
					addr.sin_addr.s_addr.copy_from_slice(ip.as_bytes());
				}
			});

			*namelen = size_of::<sockaddr_in>().try_into().unwrap();

			0
		} else {
			-EINVAL
		}
	}

	fn getsockname(&self, name: *mut sockaddr, namelen: *mut socklen_t) -> i32 {
		if namelen.is_null() {
			return -ENOBUFS;
		}

		let namelen = unsafe { &mut *namelen };
		if *namelen >= size_of::<sockaddr_in>().try_into().unwrap() {
			let addr = unsafe { &mut *(name as *mut sockaddr_in) };

			self.with(|socket| {
				let local = socket.local_endpoint();

				addr.sin_port = local.port.to_be();
				addr.sin_family = AF_INET.try_into().unwrap();

				if let IpAddress::Ipv4(ip) = local.addr {
					addr.sin_addr.s_addr.copy_from_slice(ip.as_bytes());
				}
			});

			*namelen = size_of::<sockaddr_in6>().try_into().unwrap();

			0
		} else {
			-EINVAL
		}
	}
}

impl ObjectInterface for Socket<IPv6> {
	fn bind(&self, name: *const sockaddr, namelen: socklen_t) -> i32 {
		if namelen == size_of::<sockaddr_in6>().try_into().unwrap() {
			let addr = unsafe { *(name as *const sockaddr_in6) };
			self.port.store(addr.sin6_port, Ordering::Release);
			0
		} else {
			-EINVAL
		}
	}

	fn connect(&self, name: *const sockaddr, namelen: socklen_t) -> i32 {
		if namelen == size_of::<sockaddr_in6>().try_into().unwrap() {
			let saddr = unsafe { *(name as *const sockaddr_in6) };
			let port = u16::from_be(saddr.sin6_port);
			let a0 = ((saddr.sin6_addr.s6_addr[0] as u16) << 8) | saddr.sin6_addr.s6_addr[1] as u16;
			let a1 = ((saddr.sin6_addr.s6_addr[2] as u16) << 8) | saddr.sin6_addr.s6_addr[3] as u16;
			let a2 = ((saddr.sin6_addr.s6_addr[4] as u16) << 8) | saddr.sin6_addr.s6_addr[5] as u16;
			let a3 = ((saddr.sin6_addr.s6_addr[6] as u16) << 8) | saddr.sin6_addr.s6_addr[7] as u16;
			let a4 = ((saddr.sin6_addr.s6_addr[8] as u16) << 8) | saddr.sin6_addr.s6_addr[9] as u16;
			let a5 =
				((saddr.sin6_addr.s6_addr[10] as u16) << 8) | saddr.sin6_addr.s6_addr[11] as u16;
			let a6 =
				((saddr.sin6_addr.s6_addr[12] as u16) << 8) | saddr.sin6_addr.s6_addr[13] as u16;
			let a7 =
				((saddr.sin6_addr.s6_addr[14] as u16) << 8) | saddr.sin6_addr.s6_addr[15] as u16;
			let address = IpAddress::v6(a0, a1, a2, a3, a4, a5, a6, a7);

			if self.nonblocking.load(Ordering::Acquire) {
				block_on(self.async_connect(address, port), Some(Duration::ZERO)).unwrap_or_else(
					|x| {
						if x == -ETIME {
							-EAGAIN
						} else {
							x
						}
					},
				)
			} else {
				block_on(self.async_connect(address, port), None).unwrap_or_else(|x| x)
			}
		} else {
			-EINVAL
		}
	}

	fn getpeername(&self, name: *mut sockaddr, namelen: *mut socklen_t) -> i32 {
		if namelen.is_null() {
			return -ENOBUFS;
		}

		let namelen = unsafe { &mut *namelen };
		if *namelen >= size_of::<sockaddr_in6>().try_into().unwrap() {
			let addr = unsafe { &mut *(name as *mut sockaddr_in6) };

			self.with(|socket| {
				let remote = socket.remote_endpoint();
				addr.sin6_port = remote.port.to_be();

				if let IpAddress::Ipv6(ip) = remote.addr {
					addr.sin6_addr.s6_addr.copy_from_slice(ip.as_bytes());
				}
			});

			*namelen = size_of::<sockaddr_in>().try_into().unwrap();

			0
		} else {
			-EINVAL
		}
	}

	fn getsockname(&self, name: *mut sockaddr, namelen: *mut socklen_t) -> i32 {
		if namelen.is_null() {
			return -ENOBUFS;
		}

		let namelen = unsafe { &mut *namelen };
		if *namelen >= size_of::<sockaddr_in6>().try_into().unwrap() {
			let addr = unsafe { &mut *(name as *mut sockaddr_in6) };

			self.with(|socket| {
				let local = socket.local_endpoint();

				addr.sin6_port = local.port.to_be();
				addr.sin6_family = AF_INET6.try_into().unwrap();

				if let IpAddress::Ipv6(ip) = local.addr {
					addr.sin6_addr.s6_addr.copy_from_slice(ip.as_bytes());
				}
			});

			*namelen = size_of::<sockaddr_in6>().try_into().unwrap();

			0
		} else {
			-EINVAL
		}
	}
}
