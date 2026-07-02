use alloc::sync::Arc;
use alloc::vec::Vec;
use core::future;
use core::task::Poll;

use virtio::vsock::{Hdr, Op, Type};
use virtio::{le16, le32, le64};

#[cfg(not(feature = "pci"))]
use crate::arch::kernel::mmio as hardware;
#[cfg(feature = "pci")]
use crate::drivers::pci as hardware;
use crate::errno::Errno;
use crate::executor::vsock::{ConnKey, RawSocket, VSOCK_MAP, VsockState};
use crate::fd::{self, Endpoint, Fd, ListenEndpoint, ObjectInterface, PollEvent};
use crate::io;

/// Further receives will be disallowed
pub const SHUT_RD: i32 = 0;
/// Further sends will be disallowed
pub const SHUT_WR: i32 = 1;
/// Further sends and receives will be disallowed
pub const SHUT_RDWR: i32 = 2;

#[derive(Debug)]
pub struct VsockListenEndpoint {
	pub port: u32,
	pub cid: Option<u32>,
}

impl VsockListenEndpoint {
	pub const fn new(port: u32, cid: Option<u32>) -> Self {
		Self { port, cid }
	}
}

#[derive(Debug)]
pub struct VsockEndpoint {
	pub port: u32,
	pub cid: u32,
}

impl VsockEndpoint {
	pub const fn new(port: u32, cid: u32) -> Self {
		Self { port, cid }
	}
}

pub struct Socket {
	/// The local port this socket is bound/listening on, or the synthetic
	/// ephemeral port of an outbound connection.
	port: u32,
	/// Set for sockets returned by `accept`: identifies the established
	/// connection in the executor's connection map. `None` for listeners and
	/// outbound-connect sockets, which are keyed by `port` instead.
	conn: Option<ConnKey>,
	cid: u32,
	is_nonblocking: bool,
}

impl Socket {
	pub fn new() -> Self {
		Self {
			port: 0,
			conn: None,
			cid: u32::MAX,
			is_nonblocking: false,
		}
	}

	/// Borrow this socket's `RawSocket` from the executor map, whether it is a
	/// listener/connect socket (keyed by `port`) or an accepted connection
	/// (keyed by `conn`).
	fn raw_mut<'a>(
		&self,
		guard: &'a mut crate::executor::vsock::VsockMap,
	) -> Option<&'a mut RawSocket> {
		match self.conn {
			Some(key) => guard.get_mut_connection(key),
			None => guard.get_mut_socket(self.port),
		}
	}
}

impl ObjectInterface for Socket {
	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		future::poll_fn(|cx| {
			let mut guard = VSOCK_MAP.lock();
			let raw = self.raw_mut(&mut guard).ok_or(Errno::Inval)?;

			match raw.state {
				VsockState::Shutdown | VsockState::ReceiveRequest => {
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
				VsockState::Reset => {
					// Reset is readable/writable so the next read/write
					// observes ECONNRESET, and signals error + hangup.
					let available = PollEvent::POLLIN
						| PollEvent::POLLRDNORM
						| PollEvent::POLLRDBAND
						| PollEvent::POLLOUT
						| PollEvent::POLLWRNORM
						| PollEvent::POLLWRBAND;

					Poll::Ready(Ok((event & available)
						| PollEvent::POLLERR
						| PollEvent::POLLHUP))
				}
				VsockState::Listen | VsockState::Connecting => {
					raw.rx_waker.register(cx.waker());
					raw.tx_waker.register(cx.waker());
					Poll::Pending
				}
				VsockState::Connected => {
					let mut available = PollEvent::empty();

					if !raw.buffer.is_empty() {
						// In case, we just establish a fresh connection in non-blocking mode, we try to read data.
						available.insert(
							PollEvent::POLLIN | PollEvent::POLLRDNORM | PollEvent::POLLRDBAND,
						);
					}

					let diff = raw.tx_cnt.abs_diff(raw.peer_fwd_cnt);
					if diff < raw.peer_buf_alloc {
						available.insert(
							PollEvent::POLLOUT | PollEvent::POLLWRNORM | PollEvent::POLLWRBAND,
						);
					}

					let ret = event & available;

					if ret.is_empty() {
						if event.intersects(
							PollEvent::POLLIN | PollEvent::POLLRDNORM | PollEvent::POLLRDBAND,
						) {
							raw.rx_waker.register(cx.waker());
						}

						if event.intersects(
							PollEvent::POLLOUT | PollEvent::POLLWRNORM | PollEvent::POLLWRBAND,
						) {
							raw.tx_waker.register(cx.waker());
						}

						Poll::Pending
					} else {
						Poll::Ready(Ok(ret))
					}
				}
			}
		})
		.await
	}

	async fn bind(&mut self, endpoint: ListenEndpoint) -> io::Result<()> {
		match endpoint {
			ListenEndpoint::Vsock(ep) => {
				// A socket may only listen on `VMADDR_CID_ANY` or this guest's
				// own CID. Binding to any other CID is rejected, mirroring Linux
				// `AF_VSOCK` (which returns `EADDRNOTAVAIL`).
				let cid = ep.cid.unwrap_or(u32::MAX);
				if cid != u32::MAX {
					let local_cid = hardware::get_vsock_driver().unwrap().lock().get_cid();
					if u64::from(cid) != local_cid {
						return Err(Errno::Addrnotavail);
					}
				}

				self.port = ep.port;
				self.cid = cid;
				VSOCK_MAP.lock().bind(ep.port)
			}
			#[cfg(feature = "net")]
			_ => Err(Errno::Inval),
		}
	}

	async fn connect(&mut self, endpoint: Endpoint) -> io::Result<()> {
		match endpoint {
			Endpoint::Vsock(ep) => {
				const HEADER_SIZE: usize = size_of::<Hdr>();
				let port = VSOCK_MAP.lock().connect(ep.port, ep.cid)?;
				self.port = port;
				self.cid = ep.cid;

				future::poll_fn(|cx| {
					if let Some(mut driver_guard) = hardware::get_vsock_driver().unwrap().try_lock()
					{
						let local_cid = driver_guard.get_cid();

						driver_guard.send_packet(HEADER_SIZE, |buffer| {
							let response = unsafe { &mut *buffer.as_mut_ptr().cast::<Hdr>() };

							response.src_cid = le64::from_ne(local_cid);
							response.dst_cid = le64::from_ne(ep.cid.into());
							response.src_port = le32::from_ne(port);
							response.dst_port = le32::from_ne(ep.port);
							response.len = le32::from_ne(0);
							response.type_ = le16::from_ne(Type::Stream.into());
							response.op = le16::from_ne(Op::Request.into());
							response.flags = le32::from_ne(0);
							response.buf_alloc = le32::from_ne(
								crate::executor::vsock::RAW_SOCKET_BUFFER_SIZE as u32,
							);
							response.fwd_cnt = le32::from_ne(0);
						});

						Poll::Ready(())
					} else {
						// FIXME: only wake when progress can be made
						cx.waker().wake_by_ref();
						Poll::Pending
					}
				})
				.await;

				future::poll_fn(|cx| {
					let mut guard = VSOCK_MAP.lock();
					let raw = guard.get_mut_socket(port).ok_or(Errno::Inval)?;

					match raw.state {
						VsockState::Connected => Poll::Ready(Ok(())),
						VsockState::Connecting => {
							raw.rx_waker.register(cx.waker());
							Poll::Pending
						}
						// A reset in response to our request means the peer
						// refused the connection.
						VsockState::Reset => Poll::Ready(Err(Errno::Connrefused)),
						_ => Poll::Ready(Err(Errno::Badf)),
					}
				})
				.await
			}
			#[cfg(feature = "net")]
			_ => Err(Errno::Inval),
		}
	}

	async fn getpeername(&self) -> io::Result<Option<Endpoint>> {
		let mut guard = VSOCK_MAP.lock();
		let raw = self.raw_mut(&mut guard).ok_or(Errno::Inval)?;

		Ok(Some(Endpoint::Vsock(VsockEndpoint::new(
			raw.remote_port,
			raw.remote_cid,
		))))
	}

	async fn getsockname(&self) -> io::Result<Option<Endpoint>> {
		let local_cid = hardware::get_vsock_driver().unwrap().lock().get_cid();

		Ok(Some(Endpoint::Vsock(VsockEndpoint::new(
			self.port,
			local_cid.try_into().unwrap(),
		))))
	}

	async fn listen(&mut self, backlog: i32) -> io::Result<()> {
		if backlog <= 0 {
			return Err(Errno::Inval);
		}
		VSOCK_MAP
			.lock()
			.set_backlog(self.port, usize::try_from(backlog).unwrap())
	}

	async fn accept(&mut self) -> io::Result<(Arc<async_lock::RwLock<Fd>>, Endpoint)> {
		let port = self.port;

		let (conn_key, endpoint) = future::poll_fn(|cx| {
			let mut guard = VSOCK_MAP.lock();
			let raw = guard.get_mut_socket(port).ok_or(Errno::Inval)?;

			match raw.state {
				VsockState::Listen => {
					if self.is_nonblocking {
						Poll::Ready(Err(Errno::Again))
					} else {
						raw.rx_waker.register(cx.waker());
						Poll::Pending
					}
				}
				VsockState::ReceiveRequest => {
					// Peek the head of the listener's backlog to build the
					// handshake response for the request we're about to accept.
					let Some(req) = raw.pending.front().copied() else {
						// Spurious ReceiveRequest with an empty queue: wait.
						raw.rx_waker.register(cx.waker());
						return Poll::Pending;
					};
					let fwd_cnt = raw.fwd_cnt;

					const HEADER_SIZE: usize = size_of::<Hdr>();
					let mut driver_guard = hardware::get_vsock_driver().unwrap().lock();
					let local_cid = driver_guard.get_cid();

					driver_guard.send_packet(HEADER_SIZE, |buffer| {
						let response = unsafe { &mut *buffer.as_mut_ptr().cast::<Hdr>() };

						response.src_cid = le64::from_ne(local_cid);
						response.dst_cid = le64::from_ne(req.remote_cid.into());
						response.src_port = le32::from_ne(port);
						response.dst_port = le32::from_ne(req.remote_port);
						response.len = le32::from_ne(0);
						response.type_ = le16::from_ne(Type::Stream.into());
						response.op = le16::from_ne(Op::Response.into());
						response.flags = le32::from_ne(0);
						response.buf_alloc =
							le32::from_ne(crate::executor::vsock::RAW_SOCKET_BUFFER_SIZE as u32);
						response.fwd_cnt = le32::from_ne(fwd_cnt);
					});
					drop(driver_guard);

					let endpoint = VsockEndpoint::new(req.remote_port, req.remote_cid);

					// Pop the request into the connection map. If more requests
					// remain queued, re-wake so a subsequent `accept()` drains
					// them (the listener stays in ReceiveRequest).
					let conn_key = guard.establish(port)?;
					if let Some(listener) = guard.get_mut_socket(port)
						&& !listener.pending.is_empty()
					{
						listener.rx_waker.wake();
					}

					Poll::Ready(Ok((conn_key, endpoint)))
				}
				_ => Poll::Ready(Err(Errno::Badf)),
			}
		})
		.await?;

		// Return the accepted connection as a DISTINCT Socket addressing the
		// established connection. The listener `self` is left untouched, so it
		// keeps accepting further connections on the same port.
		let conn = Socket {
			port,
			conn: Some(conn_key),
			cid: self.cid,
			is_nonblocking: self.is_nonblocking,
		};

		Ok((
			Arc::new(async_lock::RwLock::new(conn.into())),
			Endpoint::Vsock(endpoint),
		))
	}

	async fn shutdown(&self, how: i32) -> io::Result<()> {
		// Validate `how` for parity with the other socket types. This does not
		// yet emit an `Op::Shutdown` to the peer, so a remote `read` is not
		// woken with EOF by a local shutdown.
		match how {
			SHUT_RD | SHUT_WR | SHUT_RDWR => Ok(()),
			_ => Err(Errno::Inval),
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

	async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
		future::poll_fn(|cx| {
			let mut guard = VSOCK_MAP.lock();
			let raw = self.raw_mut(&mut guard).ok_or(Errno::Inval)?;

			match raw.state {
				VsockState::Connected => {
					let len = core::cmp::min(buf.len(), raw.buffer.len());

					if len == 0 {
						if self.is_nonblocking {
							Poll::Ready(Err(Errno::Again))
						} else {
							raw.rx_waker.register(cx.waker());
							Poll::Pending
						}
					} else {
						let tmp: Vec<_> = raw.buffer.drain(..len).collect();
						buf[..len].copy_from_slice(tmp.as_slice());

						Poll::Ready(Ok(len))
					}
				}
				VsockState::Shutdown | VsockState::Reset => {
					let len = core::cmp::min(buf.len(), raw.buffer.len());

					if len != 0 {
						// Deliver any data buffered before the peer closed or
						// reset the connection.
						let tmp: Vec<_> = raw.buffer.drain(..len).collect();
						buf[..len].copy_from_slice(tmp.as_slice());

						Poll::Ready(Ok(len))
					} else if raw.state == VsockState::Reset {
						// Abortive close with no remaining data: surface
						// ECONNRESET, matching Linux `AF_VSOCK`.
						Poll::Ready(Err(Errno::Connreset))
					} else {
						// Graceful shutdown, buffer drained: report EOF.
						Poll::Ready(Ok(0))
					}
				}
				// A connection-keyed socket only ever holds Connected, Shutdown,
				// or Reset; the remaining states cannot occur here. Treat them
				// as EOF defensively.
				_ => Poll::Ready(Ok(0)),
			}
		})
		.await
	}

	async fn write(&self, buf: &[u8]) -> io::Result<usize> {
		let port = self.port;
		future::poll_fn(|cx| {
			let mut guard = VSOCK_MAP.lock();
			let raw = self.raw_mut(&mut guard).ok_or(Errno::Inval)?;
			let diff = raw.tx_cnt.abs_diff(raw.peer_fwd_cnt);

			match raw.state {
				VsockState::Connected => {
					if diff >= raw.peer_buf_alloc {
						if self.is_nonblocking {
							Poll::Ready(Err(Errno::Again))
						} else {
							raw.tx_waker.register(cx.waker());
							Poll::Pending
						}
					} else {
						const HEADER_SIZE: usize = size_of::<Hdr>();
						let mut driver_guard = hardware::get_vsock_driver().unwrap().lock();
						let local_cid = driver_guard.get_cid();
						let len = core::cmp::min(
							buf.len(),
							usize::try_from(raw.peer_buf_alloc - diff).unwrap(),
						);

						driver_guard.send_packet(HEADER_SIZE + len, |virtio_buffer| {
							let response =
								unsafe { &mut *virtio_buffer.as_mut_ptr().cast::<Hdr>() };

							raw.tx_cnt = raw.tx_cnt.wrapping_add(len.try_into().unwrap());
							response.src_cid = le64::from_ne(local_cid);
							response.dst_cid = le64::from_ne(raw.remote_cid.into());
							response.src_port = le32::from_ne(port);
							response.dst_port = le32::from_ne(raw.remote_port);
							response.len = le32::from_ne(len.try_into().unwrap());
							response.type_ = le16::from_ne(Type::Stream.into());
							response.op = le16::from_ne(Op::Rw.into());
							response.flags = le32::from_ne(0);
							response.buf_alloc = le32::from_ne(
								crate::executor::vsock::RAW_SOCKET_BUFFER_SIZE as u32,
							);
							response.fwd_cnt = le32::from_ne(raw.fwd_cnt);

							virtio_buffer[HEADER_SIZE..HEADER_SIZE + len]
								.copy_from_slice(&buf[..len]);
						});

						Poll::Ready(Ok(len))
					}
				}
				// Peer reset the connection: writing fails with ECONNRESET.
				VsockState::Reset => Poll::Ready(Err(Errno::Connreset)),
				// Peer closed its receive half (graceful shutdown) or the
				// connection is otherwise gone: writing fails with EPIPE,
				// matching Linux `AF_VSOCK`.
				_ => Poll::Ready(Err(Errno::Pipe)),
			}
		})
		.await
	}
}

impl Drop for Socket {
	fn drop(&mut self) {
		// Remove our state and snapshot the peer first, releasing VSOCK_MAP
		// before touching the driver: the RX dispatch locks driver -> VSOCK_MAP,
		// so holding VSOCK_MAP while taking the driver lock would invert the
		// lock order.
		let peer = {
			let mut guard = VSOCK_MAP.lock();
			match self.conn {
				Some(key) => {
					let peer = guard
						.get_mut_connection(key)
						.map(|raw| (raw.state, raw.remote_cid, raw.remote_port, key.0));
					guard.remove_connection(key);
					peer
				}
				None => {
					let peer = guard
						.get_mut_socket(self.port)
						.map(|raw| (raw.state, raw.remote_cid, raw.remote_port, self.port));
					guard.remove_socket(self.port);
					peer
				}
			}
		};

		// Complete the close handshake: tell the peer this connection is gone.
		// Without this the peer never learns the socket died. A peer that
		// closed first (half-open) sits in its close timeout (~8 s on Linux
		// virtio-vsock) and then fires an `Op::Rst` at our local port — which
		// the ephemeral allocator may have already handed to a NEW connection.
		// A peer with the connection still open blocks in `read()` forever.
		// Only states with a live peer are notified.
		let Some((state, remote_cid, remote_port, local_port)) = peer else {
			return;
		};
		if !matches!(
			state,
			VsockState::Connected | VsockState::Shutdown | VsockState::Connecting
		) {
			return;
		}
		if let Some(driver) = hardware::get_vsock_driver() {
			const HEADER_SIZE: usize = size_of::<Hdr>();
			let mut driver_guard = driver.lock();
			let local_cid = driver_guard.get_cid();
			driver_guard.send_packet(HEADER_SIZE, |buffer| {
				let response = unsafe { &mut *buffer.as_mut_ptr().cast::<Hdr>() };

				response.src_cid = le64::from_ne(local_cid);
				response.dst_cid = le64::from_ne(remote_cid.into());
				response.src_port = le32::from_ne(local_port);
				response.dst_port = le32::from_ne(remote_port);
				response.len = le32::from_ne(0);
				response.type_ = le16::from_ne(Type::Stream.into());
				response.op = le16::from_ne(Op::Rst.into());
				response.flags = le32::from_ne(0);
				response.buf_alloc =
					le32::from_ne(crate::executor::vsock::RAW_SOCKET_BUFFER_SIZE as u32);
				response.fwd_cnt = le32::from_ne(0);
			});
		}
	}
}
