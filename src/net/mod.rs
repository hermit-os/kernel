mod device;
pub(crate) mod executor;

use alloc::boxed::Box;
use core::ops::DerefMut;
use core::str::FromStr;
use core::sync::atomic::{AtomicU16, Ordering};
use core::task::Poll;

use futures_lite::future;
use hermit_sync::InterruptTicketMutex;
use smoltcp::iface::{self, SocketHandle, SocketSet};
#[cfg(feature = "trace")]
use smoltcp::phy::Tracer;
#[cfg(feature = "dhcpv4")]
use smoltcp::socket::dhcpv4;
use smoltcp::socket::{tcp, udp, AnySocket};
use smoltcp::time::{Duration, Instant};
use smoltcp::wire::IpAddress;
#[cfg(feature = "dhcpv4")]
use smoltcp::wire::{IpCidr, Ipv4Address, Ipv4Cidr};

use crate::net::device::HermitNet;
use crate::net::executor::spawn;
use crate::{arch, DEFAULT_KEEP_ALIVE_INTERVAL};

pub(crate) enum NetworkState<'a> {
	Missing,
	InitializationFailed,
	Initialized(Box<NetworkInterface<'a>>),
}

impl<'a> NetworkState<'a> {
	pub fn as_nic_mut(&mut self) -> Result<&mut NetworkInterface<'a>, &'static str> {
		match self {
			NetworkState::Initialized(nic) => Ok(nic),
			_ => Err("Network is not initialized!"),
		}
	}
}

pub(crate) type Handle = SocketHandle;

static LOCAL_ENDPOINT: AtomicU16 = AtomicU16::new(0);
pub(crate) static NIC: InterruptTicketMutex<NetworkState<'_>> =
	InterruptTicketMutex::new(NetworkState::Missing);

pub(crate) struct NetworkInterface<'a> {
	pub iface: smoltcp::iface::Interface,
	sockets: SocketSet<'a>,
	device: HermitNet,
	#[cfg(feature = "dhcpv4")]
	dhcp_handle: SocketHandle,
}

fn start_endpoint() -> u16 {
	((unsafe { core::arch::x86_64::_rdtsc() }) % (u16::MAX as u64))
		.try_into()
		.unwrap()
}

#[inline]
pub(crate) fn now() -> Instant {
	let microseconds = arch::processor::get_timer_ticks() + arch::get_boot_time();
	Instant::from_micros_const(microseconds.try_into().unwrap())
}

async fn network_run() {
	future::poll_fn(|cx| match NIC.lock().deref_mut() {
		NetworkState::Initialized(nic) => {
			nic.poll_common(now());

			// this background task will never stop
			// => wakeup ourself
			cx.waker().clone().wake();

			Poll::Pending
		}
		_ => Poll::Ready(()),
	})
	.await
}

pub(crate) fn init() {
	info!("Try to nitialize network!");

	// initialize variable, which contains the next local endpoint
	LOCAL_ENDPOINT.store(start_endpoint(), Ordering::Relaxed);

	let mut guard = NIC.lock();

	*guard = NetworkInterface::create();

	if let NetworkState::Initialized(nic) = guard.deref_mut() {
		let time = now();
		nic.poll_common(time);
		let wakeup_time = nic
			.poll_delay(time)
			.map(|d| crate::arch::processor::get_timer_ticks() + d.total_micros());
		crate::core_scheduler().add_network_timer(wakeup_time);

		spawn(network_run()).detach();
	}
}

impl<'a> NetworkInterface<'a> {
	pub(crate) fn create_udp_handle(&mut self) -> Result<Handle, ()> {
		// Must fit mDNS payload of at least one packet
		let udp_rx_buffer =
			udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0; 1024]);
		// Will not send mDNS
		let udp_tx_buffer = udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY], vec![0; 0]);
		let udp_socket = udp::Socket::new(udp_rx_buffer, udp_tx_buffer);
		let udp_handle = self.sockets.add(udp_socket);

		Ok(udp_handle)
	}

	pub(crate) fn create_tcp_handle(&mut self) -> Result<Handle, ()> {
		let tcp_rx_buffer = tcp::SocketBuffer::new(vec![0; 65535]);
		let tcp_tx_buffer = tcp::SocketBuffer::new(vec![0; 65535]);
		let mut tcp_socket = tcp::Socket::new(tcp_rx_buffer, tcp_tx_buffer);
		tcp_socket.set_nagle_enabled(true);
		let tcp_handle = self.sockets.add(tcp_socket);

		Ok(tcp_handle)
	}

	pub(crate) fn poll_common(&mut self, timestamp: Instant) {
		let _ = self
			.iface
			.poll(timestamp, &mut self.device, &mut self.sockets);

		#[cfg(feature = "dhcpv4")]
		match self
			.sockets
			.get_mut::<dhcpv4::Socket<'_>>(self.dhcp_handle)
			.poll()
		{
			None => {}
			Some(dhcpv4::Event::Configured(config)) => {
				info!("DHCP config acquired!");
				info!("IP address:      {}", config.address);
				self.iface.update_ip_addrs(|addrs| {
					if let Some(dest) = addrs.iter_mut().next() {
						*dest = IpCidr::Ipv4(config.address);
					} else {
						if !addrs.push(IpCidr::Ipv4(config.address)).is_ok() {
							info!("Unable to update IP address");
						}
					}
				});
				if let Some(router) = config.router {
					info!("Default gateway: {}", router);
					self.iface
						.routes_mut()
						.add_default_ipv4_route(router)
						.unwrap();
				} else {
					info!("Default gateway: None");
					self.iface.routes_mut().remove_default_ipv4_route();
				}

				for (i, s) in config.dns_servers.iter().enumerate() {
					info!("DNS server {}:    {}", i, s);
				}
			}
			Some(dhcpv4::Event::Deconfigured) => {
				info!("DHCP lost config!");
				let cidr = Ipv4Cidr::new(Ipv4Address::UNSPECIFIED, 0);
				self.iface.update_ip_addrs(|addrs| {
					if let Some(dest) = addrs.iter_mut().next() {
						*dest = IpCidr::Ipv4(cidr);
					}
				});
				self.iface.routes_mut().remove_default_ipv4_route();
			}
		};
	}

	pub(crate) fn poll_delay(&mut self, timestamp: Instant) -> Option<Duration> {
		self.iface.poll_delay(timestamp, &self.sockets)
	}

	#[allow(dead_code)]
	pub(crate) fn get_socket<T: AnySocket<'a>>(&self, handle: SocketHandle) -> &T {
		self.sockets.get(handle)
	}

	pub(crate) fn get_mut_socket<T: AnySocket<'a>>(&mut self, handle: SocketHandle) -> &mut T {
		self.sockets.get_mut(handle)
	}

	pub(crate) fn get_socket_and_context<T: AnySocket<'a>>(
		&mut self,
		handle: SocketHandle,
	) -> (&mut T, &mut smoltcp::iface::Context) {
		(self.sockets.get_mut(handle), self.iface.context())
	}
}

pub(crate) struct AsyncSocket(Handle);

impl AsyncSocket {
	pub(crate) fn new() -> Self {
		let handle = NIC
			.lock()
			.as_nic_mut()
			.unwrap()
			.create_tcp_handle()
			.unwrap();
		Self(handle)
	}

	pub(crate) fn inner(&self) -> Handle {
		self.0
	}

	fn with<R>(&self, f: impl FnOnce(&mut tcp::Socket<'_>) -> R) -> R {
		let mut guard = NIC.lock();
		let nic = guard.as_nic_mut().unwrap();
		let res = {
			let s = nic.get_mut_socket::<tcp::Socket<'_>>(self.0);
			f(s)
		};
		let t = now();
		if nic.poll_delay(t).map(|d| d.total_millis()).unwrap_or(0) == 0 {
			nic.poll_common(t);
		}
		res
	}

	fn with_context<R>(&self, f: impl FnOnce(&mut tcp::Socket<'_>, &mut iface::Context) -> R) -> R {
		let mut guard = NIC.lock();
		let nic = guard.as_nic_mut().unwrap();
		let res = {
			let (s, cx) = nic.get_socket_and_context::<tcp::Socket<'_>>(self.0);
			f(s, cx)
		};
		let t = now();
		if nic.poll_delay(t).map(|d| d.total_millis()).unwrap_or(0) == 0 {
			nic.poll_common(t);
		}
		res
	}

	pub(crate) async fn connect(&self, ip: &[u8], port: u16) -> Result<Handle, i32> {
		let address =
			IpAddress::from_str(core::str::from_utf8(ip).map_err(|_| -crate::errno::EIO)?)
				.map_err(|_| -crate::errno::EIO)?;

		self.with_context(|socket, cx| {
			socket.connect(
				cx,
				(address, port),
				LOCAL_ENDPOINT.fetch_add(1, Ordering::SeqCst),
			)
		})
		.map_err(|_| -crate::errno::EIO)?;

		future::poll_fn(|cx| {
			self.with(|socket| match socket.state() {
				tcp::State::Closed | tcp::State::TimeWait => {
					Poll::Ready(Err(-crate::errno::EFAULT))
				}
				tcp::State::Listen => Poll::Ready(Err(-crate::errno::EIO)),
				tcp::State::SynSent | tcp::State::SynReceived => {
					socket.register_send_waker(cx.waker());
					Poll::Pending
				}
				_ => Poll::Ready(Ok(self.0)),
			})
		})
		.await
	}

	pub(crate) async fn accept(&self, port: u16) -> Result<(IpAddress, u16), i32> {
		self.with(|socket| socket.listen(port).map_err(|_| -crate::errno::EIO))?;

		future::poll_fn(|cx| {
			self.with(|socket| {
				if socket.is_active() {
					Poll::Ready(Ok(()))
				} else {
					match socket.state() {
						tcp::State::Closed
						| tcp::State::Closing
						| tcp::State::FinWait1
						| tcp::State::FinWait2 => Poll::Ready(Err(-crate::errno::EIO)),
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
		let socket = nic.get_mut_socket::<tcp::Socket<'_>>(self.0);
		socket.set_keep_alive(Some(Duration::from_millis(DEFAULT_KEEP_ALIVE_INTERVAL)));
		let endpoint = socket.remote_endpoint().ok_or(-crate::errno::EIO)?;

		Ok((endpoint.addr, endpoint.port))
	}

	pub(crate) async fn read(&self, buffer: &mut [u8]) -> Result<usize, i32> {
		future::poll_fn(|cx| {
			self.with(|socket| match socket.state() {
				tcp::State::FinWait1
				| tcp::State::FinWait2
				| tcp::State::Closed
				| tcp::State::Closing
				| tcp::State::TimeWait => Poll::Ready(Err(-crate::errno::EIO)),
				_ => {
					if socket.can_recv() {
						let n = socket.recv_slice(buffer).map_err(|_| -crate::errno::EIO)?;
						if n > 0 || buffer.is_empty() {
							return Poll::Ready(Ok(n));
						}
					}

					socket.register_recv_waker(cx.waker());
					Poll::Pending
				}
			})
		})
		.await
	}

	pub(crate) async fn write(&self, buffer: &[u8]) -> Result<usize, i32> {
		let len = buffer.len();
		let mut pos: usize = 0;

		while pos < len {
			let n = future::poll_fn(|cx| {
				self.with(|socket| match socket.state() {
					tcp::State::FinWait1
					| tcp::State::FinWait2
					| tcp::State::Closed
					| tcp::State::Closing
					| tcp::State::TimeWait => Poll::Ready(Err(-crate::errno::EIO)),
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
				return Ok(pos);
			}

			pos += n;
		}

		Ok(pos)
	}

	pub(crate) async fn close(&self) -> Result<(), i32> {
		future::poll_fn(|cx| {
			self.with(|socket| match socket.state() {
				tcp::State::FinWait1
				| tcp::State::FinWait2
				| tcp::State::Closed
				| tcp::State::Closing
				| tcp::State::TimeWait => Poll::Ready(Err(-crate::errno::EIO)),
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
}

impl From<Handle> for AsyncSocket {
	fn from(handle: Handle) -> Self {
		AsyncSocket(handle)
	}
}
