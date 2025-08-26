use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::future;
use core::task::Poll;

use async_trait::async_trait;
use virtio::vsock::{Hdr, Op, Type};
use virtio::{le16, le32, le64};

#[cfg(not(feature = "pci"))]
use crate::arch::kernel::mmio as hardware;
#[cfg(feature = "pci")]
use crate::drivers::pci as hardware;
use crate::errno::Errno;
use crate::executor::vsock::{VSOCK_MAP, VsockState};
use crate::fd::{self, Endpoint, ListenEndpoint, ObjectInterface, PollEvent};
use crate::io;

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

#[derive(Debug)]
pub struct NullSocket;

impl NullSocket {
	pub const fn new() -> Self {
		Self {}
	}
}

#[async_trait]
impl ObjectInterface for NullSocket {}

#[derive(Debug)]
pub struct Socket {
	port: u32,
	cid: u32,
	is_nonblocking: bool,
}

impl Socket {
	pub fn new() -> Self {
		Self {
			port: 0,
			cid: u32::MAX,
			is_nonblocking: false,
		}
	}

	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		future::poll_fn(|cx| {
			let mut guard = VSOCK_MAP.lock();
			let raw = guard.get_mut_socket(self.port).ok_or(Errno::Inval)?;

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
				self.port = ep.port;
				if let Some(cid) = ep.cid {
					self.cid = cid;
				} else {
					self.cid = u32::MAX;
				}
				VSOCK_MAP.lock().bind(ep.port)
			}
			#[cfg(feature = "net")]
			_ => Err(Errno::Inval),
		}
	}

	async fn connect(&mut self, endpoint: Endpoint) -> io::Result<()> {
		match endpoint {
			Endpoint::Vsock(ep) => {
				const HEADER_SIZE: usize = core::mem::size_of::<Hdr>();
				let port = VSOCK_MAP.lock().connect(ep.port, ep.cid)?;
				self.port = port;
				self.port = ep.cid;

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
		let guard = VSOCK_MAP.lock();
		let raw = guard.get_socket(self.port).ok_or(Errno::Inval)?;

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

	async fn listen(&mut self, _backlog: i32) -> io::Result<()> {
		Ok(())
	}

	async fn accept(&mut self) -> io::Result<(NullSocket, Endpoint)> {
		let port = self.port;
		let cid = self.cid;

		let endpoint = future::poll_fn(|cx| {
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
					let result = {
						const HEADER_SIZE: usize = core::mem::size_of::<Hdr>();
						let mut driver_guard = hardware::get_vsock_driver().unwrap().lock();
						let local_cid = driver_guard.get_cid();

						driver_guard.send_packet(HEADER_SIZE, |buffer| {
							let response = unsafe { &mut *buffer.as_mut_ptr().cast::<Hdr>() };

							response.src_cid = le64::from_ne(local_cid);
							response.dst_cid = le64::from_ne(raw.remote_cid.into());
							response.src_port = le32::from_ne(port);
							response.dst_port = le32::from_ne(raw.remote_port);
							response.len = le32::from_ne(0);
							response.type_ = le16::from_ne(Type::Stream.into());
							if local_cid != u64::from(cid) && cid != u32::MAX {
								response.op = le16::from_ne(Op::Rst.into());
							} else {
								response.op = le16::from_ne(Op::Response.into());
							}
							response.flags = le32::from_ne(0);
							response.buf_alloc = le32::from_ne(
								crate::executor::vsock::RAW_SOCKET_BUFFER_SIZE as u32,
							);
							response.fwd_cnt = le32::from_ne(raw.fwd_cnt);
						});

						raw.state = VsockState::Connected;

						Ok(VsockEndpoint::new(raw.remote_port, raw.remote_cid))
					};

					Poll::Ready(result)
				}
				_ => Poll::Ready(Err(Errno::Badf)),
			}
		})
		.await?;

		Ok((NullSocket::new(), Endpoint::Vsock(endpoint)))
	}

	async fn shutdown(&self, _how: i32) -> io::Result<()> {
		Ok(())
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

	async fn read(&self, buffer: &mut [u8]) -> io::Result<usize> {
		let port = self.port;
		future::poll_fn(|cx| {
			let mut guard = VSOCK_MAP.lock();
			let raw = guard.get_mut_socket(port).ok_or(Errno::Inval)?;

			match raw.state {
				VsockState::Connected => {
					let len = core::cmp::min(buffer.len(), raw.buffer.len());

					if len == 0 {
						if self.is_nonblocking {
							Poll::Ready(Err(Errno::Again))
						} else {
							raw.rx_waker.register(cx.waker());
							Poll::Pending
						}
					} else {
						let tmp: Vec<_> = raw.buffer.drain(..len).collect();
						buffer[..len].copy_from_slice(tmp.as_slice());

						Poll::Ready(Ok(len))
					}
				}
				VsockState::Shutdown => {
					let len = core::cmp::min(buffer.len(), raw.buffer.len());

					if len == 0 {
						Poll::Ready(Ok(0))
					} else {
						let tmp: Vec<_> = raw.buffer.drain(..len).collect();
						buffer[..len].copy_from_slice(tmp.as_slice());

						Poll::Ready(Ok(len))
					}
				}
				_ => Poll::Ready(Err(Errno::Io)),
			}
		})
		.await
	}

	async fn write(&self, buffer: &[u8]) -> io::Result<usize> {
		let port = self.port;
		future::poll_fn(|cx| {
			let mut guard = VSOCK_MAP.lock();
			let raw = guard.get_mut_socket(port).ok_or(Errno::Inval)?;
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
						const HEADER_SIZE: usize = core::mem::size_of::<Hdr>();
						let mut driver_guard = hardware::get_vsock_driver().unwrap().lock();
						let local_cid = driver_guard.get_cid();
						let len = core::cmp::min(
							buffer.len(),
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
								.copy_from_slice(&buffer[..len]);
						});

						Poll::Ready(Ok(len))
					}
				}
				_ => Poll::Ready(Err(Errno::Io)),
			}
		})
		.await
	}
}

impl Drop for Socket {
	fn drop(&mut self) {
		let mut guard = VSOCK_MAP.lock();
		guard.remove_socket(self.port);
	}
}

#[async_trait]
impl ObjectInterface for Socket {
	async fn poll(&self, event: PollEvent) -> io::Result<PollEvent> {
		self.poll(event).await
	}

	async fn read(&self, buffer: &mut [u8]) -> io::Result<usize> {
		self.read(buffer).await
	}

	async fn write(&self, buffer: &[u8]) -> io::Result<usize> {
		self.write(buffer).await
	}

	async fn bind(&mut self, endpoint: ListenEndpoint) -> io::Result<()> {
		self.bind(endpoint).await
	}

	async fn connect(&mut self, endpoint: Endpoint) -> io::Result<()> {
		self.connect(endpoint).await
	}

	async fn accept(
		&mut self,
	) -> io::Result<(Arc<async_lock::RwLock<dyn ObjectInterface>>, Endpoint)> {
		let (handle, endpoint) = self.accept().await?;
		Ok((Arc::new(async_lock::RwLock::new(handle)), endpoint))
	}

	async fn getpeername(&self) -> io::Result<Option<Endpoint>> {
		self.getpeername().await
	}

	async fn getsockname(&self) -> io::Result<Option<Endpoint>> {
		self.getsockname().await
	}

	async fn listen(&mut self, backlog: i32) -> io::Result<()> {
		self.listen(backlog).await
	}

	async fn shutdown(&self, how: i32) -> io::Result<()> {
		self.shutdown(how).await
	}

	async fn status_flags(&self) -> io::Result<fd::StatusFlags> {
		self.status_flags().await
	}

	async fn set_status_flags(&mut self, status_flags: fd::StatusFlags) -> io::Result<()> {
		self.set_status_flags(status_flags).await
	}
}
