use alloc::boxed::Box;
#[cfg(feature = "dns")]
use alloc::vec::Vec;
use core::future;
use core::sync::atomic::{AtomicU16, Ordering};
use core::task::Poll;

use hermit_sync::InterruptTicketMutex;
use smoltcp::iface::{PollResult, SocketHandle, SocketSet};
use smoltcp::socket::AnySocket;
#[cfg(feature = "dhcpv4")]
use smoltcp::socket::dhcpv4;
#[cfg(feature = "dns")]
use smoltcp::socket::dns::{self, GetQueryResultError, QueryHandle};
#[cfg(feature = "tcp")]
use smoltcp::socket::tcp;
#[cfg(feature = "udp")]
use smoltcp::socket::udp;
use smoltcp::time::{Duration, Instant};
#[cfg(feature = "dns")]
use smoltcp::wire::{DnsQueryType, IpAddress};
#[cfg(feature = "dhcpv4")]
use smoltcp::wire::{IpCidr, Ipv4Address, Ipv4Cidr};

use crate::arch;
use crate::drivers::net::{NetworkDevice, NetworkDriver};
#[cfg(feature = "dns")]
use crate::errno::Errno;
use crate::executor::{WakerRegistration, spawn};
#[cfg(feature = "dns")]
use crate::io;
use crate::scheduler::timer_interrupts::{Source, create_timer};

pub(crate) enum NetworkState<'a> {
	Missing,
	// Never constructed if the kernel is configured for the loopback driver.
	#[allow(dead_code)]
	InitializationFailed,
	Initialized(Box<NetworkInterface<'a>>),
}

#[cfg(any(
	all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci")),
	feature = "rtl8139",
	feature = "virtio-net",
))]
pub(crate) fn network_handler() {
	NIC.lock().as_nic_mut().unwrap().handle_interrupt();
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
	pub(super) iface: smoltcp::iface::Interface,
	pub(super) sockets: SocketSet<'a>,
	#[cfg(feature = "trace")]
	pub(super) device: smoltcp::phy::Tracer<NetworkDevice>,
	#[cfg(not(feature = "trace"))]
	pub(super) device: NetworkDevice,
	#[cfg(feature = "dhcpv4")]
	pub(super) dhcp_handle: SocketHandle,
	#[cfg(feature = "dns")]
	pub(super) dns_handle: Option<SocketHandle>,
}

#[cfg(target_arch = "x86_64")]
fn start_endpoint() -> u16 {
	((unsafe { core::arch::x86_64::_rdtsc() }) % u64::from(u16::MAX))
		.try_into()
		.unwrap()
}

#[cfg(target_arch = "aarch64")]
fn start_endpoint() -> u16 {
	use core::arch::asm;
	let value: u64;

	unsafe {
		asm!(
			"mrs {value}, cntpct_el0",
			value = out(reg) value,
			options(nostack),
		);
	}

	(value % u64::from(u16::MAX)).try_into().unwrap()
}

#[cfg(target_arch = "riscv64")]
fn start_endpoint() -> u16 {
	(riscv::register::time::read64() % u64::from(u16::MAX))
		.try_into()
		.unwrap()
}

#[inline]
pub(crate) fn now() -> Instant {
	Instant::from_micros_const(arch::kernel::systemtime::now_micros().try_into().unwrap())
}

#[cfg(feature = "dhcpv4")]
async fn dhcpv4_run() {
	future::poll_fn(|cx| {
		let Some(mut guard) = NIC.try_lock() else {
			// FIXME: only wake when progress can be made
			cx.waker().wake_by_ref();
			return Poll::Pending;
		};

		let nic = guard.as_nic_mut().unwrap();
		let dhcp_handle = nic.dhcp_handle;
		let socket = nic.sockets.get_mut::<dhcpv4::Socket<'_>>(dhcp_handle);

		socket.register_waker(cx.waker());

		match socket.poll() {
			None => {}
			Some(dhcpv4::Event::Configured(config)) => {
				info!("DHCP config acquired!");
				info!("IP address:   {}", config.address);
				nic.iface.update_ip_addrs(|addrs| {
					if let Some(dest) = addrs.iter_mut().next() {
						*dest = IpCidr::Ipv4(config.address);
					} else if addrs.push(IpCidr::Ipv4(config.address)).is_err() {
						info!("Unable to update IP address");
					}
				});
				if let Some(router) = config.router {
					info!("Gateway:      {router}");
					nic.iface
						.routes_mut()
						.add_default_ipv4_route(router)
						.unwrap();
				} else {
					info!("Gateway:      None");
					nic.iface.routes_mut().remove_default_ipv4_route();
				}

				#[cfg(feature = "dns")]
				let mut dns_servers: Vec<IpAddress> = Vec::new();
				for (i, s) in config.dns_servers.iter().enumerate() {
					info!("DNS server {i}: {s}");
					#[cfg(feature = "dns")]
					dns_servers.push(IpAddress::Ipv4(*s));
				}

				#[cfg(feature = "dns")]
				if !dns_servers.is_empty() {
					let dns_socket = dns::Socket::new(dns_servers.as_slice(), vec![]);
					nic.dns_handle = Some(nic.sockets.add(dns_socket));
				}
			}
			Some(dhcpv4::Event::Deconfigured) => {
				info!("DHCP lost config!");
				let cidr = Ipv4Cidr::new(Ipv4Address::UNSPECIFIED, 0);
				nic.iface.update_ip_addrs(|addrs| {
					if let Some(dest) = addrs.iter_mut().next() {
						*dest = IpCidr::Ipv4(cidr);
					}
				});
				nic.iface.routes_mut().remove_default_ipv4_route();

				#[cfg(feature = "dns")]
				{
					if let Some(dns_handle) = nic.dns_handle {
						nic.sockets.remove(dns_handle);
					}

					nic.dns_handle = None;
				}
			}
		};

		Poll::<()>::Pending
	})
	.await;
}

pub(crate) static NETWORK_WAKER: InterruptTicketMutex<WakerRegistration> =
	InterruptTicketMutex::new(WakerRegistration::new());

#[track_caller]
pub(crate) fn wake_network_waker() {
	if log_enabled!(log::Level::Trace) {
		let module = core::panic::Location::caller()
			.file()
			.rsplit('/')
			.map(|m| m.split_once('.').map_or(m, |i| i.0))
			.find(|m| *m != "mod")
			.unwrap();
		trace!(target: module, "Waking network waker");
	}

	NETWORK_WAKER.lock().wake();
}

async fn network_run() {
	future::poll_fn(|cx| {
		if let Some(mut guard) = NIC.try_lock() {
			match &mut *guard {
				NetworkState::Initialized(nic) => {
					let now = now();

					match nic.poll_common(now) {
						PollResult::SocketStateChanged => {
							// Progress was made
							cx.waker().wake_by_ref();
						}
						PollResult::None => {
							// Very likely no progress can be made, so set up a timer interrupt to wake the waker
							NETWORK_WAKER.lock().register(cx.waker());
							nic.set_polling_mode(false);
							if let Some(wakeup_time) = nic.poll_delay(now).map(|d| d.total_micros())
							{
								create_timer(Source::Network, wakeup_time);
								trace!("Configured an interrupt for {wakeup_time:?}");
							}
						}
					}

					Poll::Pending
				}
				_ => Poll::Ready(()),
			}
		} else {
			// FIXME: only wake when progress can be made
			cx.waker().wake_by_ref();
			// another task is already using the NIC => don't check
			Poll::Pending
		}
	})
	.await;
}

#[cfg(feature = "dns")]
pub(crate) async fn get_query_result(query: QueryHandle) -> io::Result<Vec<IpAddress>> {
	future::poll_fn(|cx| {
		let Some(mut guard) = NIC.try_lock() else {
			// FIXME: only wake when progress can be made
			cx.waker().wake_by_ref();
			return Poll::Pending;
		};

		let nic = guard.as_nic_mut().unwrap();
		let socket = nic.get_mut_dns_socket()?;
		match socket.get_query_result(query) {
			Ok(addrs) => {
				let mut ips = Vec::new();
				for x in &addrs {
					ips.push(*x);
				}

				Poll::Ready(Ok(ips))
			}
			Err(GetQueryResultError::Pending) => {
				socket.register_query_waker(query, cx.waker());
				Poll::Pending
			}
			Err(e) => {
				warn!("DNS query failed: {e:?}");
				Poll::Ready(Err(Errno::Noent))
			}
		}
	})
	.await
}

pub(crate) fn init() {
	info!("Try to initialize network!");

	// initialize variable, which contains the next local endpoint
	LOCAL_ENDPOINT.store(start_endpoint(), Ordering::Relaxed);

	let mut guard = NIC.lock();

	*guard = NetworkInterface::create();

	if let NetworkState::Initialized(_) = &mut *guard {
		spawn(network_run());
		#[cfg(feature = "dhcpv4")]
		spawn(dhcpv4_run());
	}
}

impl<'a> NetworkInterface<'a> {
	#[cfg(feature = "udp")]
	pub(crate) fn create_udp_handle(&mut self) -> Result<Handle, ()> {
		let udp_rx_buffer =
			udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0; 0x10000]);
		let udp_tx_buffer =
			udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0; 0x10000]);
		let udp_socket = udp::Socket::new(udp_rx_buffer, udp_tx_buffer);
		let udp_handle = self.sockets.add(udp_socket);

		Ok(udp_handle)
	}

	#[cfg(feature = "tcp")]
	pub(crate) fn create_tcp_handle(&mut self) -> Result<Handle, ()> {
		let tcp_rx_buffer = tcp::SocketBuffer::new(vec![0; 0x10000]);
		let tcp_tx_buffer = tcp::SocketBuffer::new(vec![0; 0x10000]);
		let mut tcp_socket = tcp::Socket::new(tcp_rx_buffer, tcp_tx_buffer);
		tcp_socket.set_nagle_enabled(true);
		let tcp_handle = self.sockets.add(tcp_socket);

		Ok(tcp_handle)
	}

	pub(crate) fn poll_common(&mut self, timestamp: Instant) -> PollResult {
		self.iface
			.poll(timestamp, &mut self.device, &mut self.sockets)
	}

	pub(crate) fn poll_delay(&mut self, timestamp: Instant) -> Option<Duration> {
		self.iface.poll_delay(timestamp, &self.sockets)
	}

	pub(crate) fn get_mut_socket<T: AnySocket<'a>>(&mut self, handle: SocketHandle) -> &mut T {
		self.sockets.get_mut(handle)
	}

	#[cfg(feature = "tcp")]
	pub(crate) fn get_socket_and_context<T: AnySocket<'a>>(
		&mut self,
		handle: SocketHandle,
	) -> (&mut T, &mut smoltcp::iface::Context) {
		(self.sockets.get_mut(handle), self.iface.context())
	}

	pub(crate) fn destroy_socket(&mut self, handle: Handle) {
		// This deallocates the socket's buffers
		self.sockets.remove(handle);
	}

	#[cfg(feature = "dns")]
	pub(crate) fn start_query(
		&mut self,
		name: &str,
		query_type: DnsQueryType,
	) -> io::Result<QueryHandle> {
		let dns_handle = self.dns_handle.ok_or(Errno::Inval)?;
		let socket: &mut dns::Socket<'a> = self.sockets.get_mut(dns_handle);
		socket
			.start_query(self.iface.context(), name, query_type)
			.map_err(|_| Errno::Io)
	}

	#[cfg(feature = "dns")]
	pub(crate) fn get_mut_dns_socket(&mut self) -> io::Result<&mut dns::Socket<'a>> {
		let dns_handle = self.dns_handle.ok_or(Errno::Inval)?;
		Ok(self.sockets.get_mut(dns_handle))
	}

	#[cfg(any(
		all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci")),
		feature = "rtl8139",
		feature = "virtio-net",
	))]
	fn handle_interrupt(&mut self) {
		#[cfg(feature = "trace")]
		self.device.get_mut().handle_interrupt();
		#[cfg(not(feature = "trace"))]
		self.device.handle_interrupt();
	}

	pub(crate) fn set_polling_mode(&mut self, value: bool) {
		#[cfg(feature = "trace")]
		self.device.get_mut().set_polling_mode(value);
		#[cfg(not(feature = "trace"))]
		self.device.set_polling_mode(value);
	}
}
