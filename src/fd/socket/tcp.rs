use core::ffi::c_void;
use core::future;
use core::ops::DerefMut;
use core::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use core::task::Poll;

use smoltcp::iface;
use smoltcp::socket::tcp;
use smoltcp::time::Duration;
use smoltcp::wire::{IpEndpoint, IpListenEndpoint};

use crate::executor::network::{block_on, now, poll_on, Handle, NetworkState, NIC};
use crate::fd::{IoError, ObjectInterface, SocketOption};
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
pub struct Socket {
	handle: Handle,
	port: AtomicU16,
	nonblocking: AtomicBool,
}

impl Socket {
	pub fn new(handle: Handle) -> Self {
		Self {
			handle,
			port: AtomicU16::new(0),
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

	// TODO: Remove allow once fixed:
	// https://github.com/rust-lang/rust-clippy/issues/11380
	#[allow(clippy::needless_pass_by_ref_mut)]
	async fn async_read(&self, buffer: &mut [u8]) -> Result<isize, IoError> {
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
									(len, isize::try_from(len).unwrap())
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

	async fn async_write(&self, buffer: &[u8]) -> Result<isize, IoError> {
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

		Ok(pos.try_into().unwrap())
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
				tcp::State::Listen => Poll::Ready(()),
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

impl ObjectInterface for Socket {
	fn bind(&self, endpoint: IpListenEndpoint) -> Result<(), IoError> {
		self.port.store(endpoint.port, Ordering::Release);
		Ok(())
	}

	fn connect(&self, endpoint: IpEndpoint) -> Result<(), IoError> {
		if self.nonblocking.load(Ordering::Acquire) {
			block_on(self.async_connect(endpoint), Some(Duration::ZERO)).map_err(|x| {
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
		block_on(self.async_accept(), None)
	}

	fn getpeername(&self) -> Option<IpEndpoint> {
		self.with(|socket| socket.remote_endpoint())
	}

	fn getsockname(&self) -> Option<IpEndpoint> {
		self.with(|socket| socket.local_endpoint())
	}

	fn read(&self, buf: &mut [u8]) -> Result<isize, IoError> {
		if buf.is_empty() {
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
		if buf.is_empty() {
			return Ok(0);
		}

		if self.nonblocking.load(Ordering::Acquire) {
			poll_on(self.async_write(buf), Some(Duration::ZERO)).map_err(|x| {
				if x == IoError::ETIME {
					IoError::EAGAIN
				} else {
					x
				}
			})
		} else {
			poll_on(self.async_write(buf), None)
		}
	}

	fn listen(&self, _backlog: i32) -> Result<(), IoError> {
		self.with(|socket| {
			if !socket.is_open() {
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

impl Clone for Socket {
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
		}
	}
}

impl Drop for Socket {
	fn drop(&mut self) {
		let _ = block_on(self.async_close(), None);
		NIC.lock().as_nic_mut().unwrap().destroy_socket(self.handle);
	}
}
