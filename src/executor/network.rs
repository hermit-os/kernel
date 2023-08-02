use alloc::boxed::Box;
use alloc::sync::Arc;
use core::future;
use core::future::Future;
use core::ops::DerefMut;
use core::sync::atomic::{AtomicU16, Ordering};
use core::task::{Context, Poll};

use crossbeam_utils::Backoff;
use hermit_sync::{without_interrupts, InterruptTicketMutex};
use smoltcp::iface::{SocketHandle, SocketSet};
#[cfg(feature = "dhcpv4")]
use smoltcp::socket::dhcpv4;
use smoltcp::socket::{tcp, udp, AnySocket};
use smoltcp::time::{Duration, Instant};
#[cfg(feature = "dhcpv4")]
use smoltcp::wire::{IpCidr, Ipv4Address, Ipv4Cidr};

use crate::arch::core_local::*;
use crate::arch::{self, interrupts};
#[cfg(not(feature = "pci"))]
use crate::drivers::mmio::get_network_driver;
use crate::drivers::net::NetworkDriver;
#[cfg(feature = "pci")]
use crate::drivers::pci::get_network_driver;
use crate::executor::device::HermitNet;
use crate::executor::{spawn, TaskNotify};

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
	pub(super) iface: smoltcp::iface::Interface,
	pub(super) sockets: SocketSet<'a>,
	pub(super) device: HermitNet,
	#[cfg(feature = "dhcpv4")]
	pub(super) dhcp_handle: SocketHandle,
}

#[cfg(target_arch = "x86_64")]
fn start_endpoint() -> u16 {
	((unsafe { core::arch::x86_64::_rdtsc() }) % (u16::MAX as u64))
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

	(value % (u16::MAX as u64)).try_into().unwrap()
}

#[inline]
pub(crate) fn now() -> Instant {
	let microseconds = arch::processor::get_timer_ticks() + arch::get_boot_time();
	Instant::from_micros_const(microseconds.try_into().unwrap())
}

async fn network_run() {
	future::poll_fn(|_cx| match NIC.lock().deref_mut() {
		NetworkState::Initialized(nic) => {
			nic.poll_common(now());
			Poll::Pending
		}
		_ => Poll::Ready(()),
	})
	.await
}

pub(crate) fn init() {
	info!("Try to initialize network!");

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

		spawn(network_run());
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
					} else if addrs.push(IpCidr::Ipv4(config.address)).is_err() {
						info!("Unable to update IP address");
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

#[inline]
fn network_delay(timestamp: Instant) -> Option<Duration> {
	crate::executor::network::NIC
		.lock()
		.as_nic_mut()
		.unwrap()
		.poll_delay(timestamp)
}

#[inline]
fn network_poll(timestamp: Instant) {
	crate::executor::network::NIC
		.lock()
		.as_nic_mut()
		.unwrap()
		.poll_common(timestamp);
}

/// Blocks the current thread on `f`, running the executor when idling.
pub(crate) fn block_on<F, T>(future: F, timeout: Option<Duration>) -> Result<T, i32>
where
	F: Future<Output = Result<T, i32>>,
{
	// allow network interrupts
	get_network_driver().unwrap().lock().set_polling_mode(true);

	let backoff = Backoff::new();
	let mut blocking_time = 1000;
	let start = now();
	let task_notify = Arc::new(TaskNotify::new());
	let waker = task_notify.into();
	let mut cx = Context::from_waker(&waker);
	let mut future = future;
	let mut future = unsafe { core::pin::Pin::new_unchecked(&mut future) };

	loop {
		// run background tasks
		crate::executor::run();

		if let Poll::Ready(t) = future.as_mut().poll(&mut cx) {
			let network_timer = network_delay(crate::executor::network::now())
				.map(|d| crate::arch::processor::get_timer_ticks() + d.total_micros());
			core_scheduler().add_network_timer(network_timer);

			// allow network interrupts
			get_network_driver().unwrap().lock().set_polling_mode(false);

			return t;
		}

		if let Some(duration) = timeout {
			if crate::executor::network::now() >= start + duration {
				let network_timer = network_delay(crate::executor::network::now())
					.map(|d| crate::arch::processor::get_timer_ticks() + d.total_micros());
				core_scheduler().add_network_timer(network_timer);

				// allow network interrupts
				get_network_driver().unwrap().lock().set_polling_mode(false);

				return Err(-crate::errno::ETIME);
			}
		}

		// disable all interrupts
		interrupts::disable();
		let now = crate::executor::network::now();
		let delay = network_delay(now).map(|d| d.total_micros());
		if backoff.is_completed() && delay.unwrap_or(10_000_000) > 10_000 {
			// add additional check before the task will block
			if let Poll::Ready(t) = future.as_mut().poll(&mut cx) {
				// allow network interrupts
				get_network_driver().unwrap().lock().set_polling_mode(false);
				// enable interrupts
				interrupts::enable();

				return t;
			}

			let ticks = crate::arch::processor::get_timer_ticks();
			let wakeup_time = timeout
				.map(|duration| {
					core::cmp::min(
						u64::try_from((start + duration).total_micros()).unwrap(),
						ticks + delay.unwrap_or(blocking_time),
					)
				})
				.or(Some(ticks + delay.unwrap_or(blocking_time)));
			let network_timer = delay.map(|d| ticks + d);
			let core_scheduler = core_scheduler();
			blocking_time *= 2;

			core_scheduler.add_network_timer(network_timer);
			core_scheduler.block_current_task(wakeup_time);

			// allow network interrupts
			get_network_driver().unwrap().lock().set_polling_mode(false);

			// enable interrupts
			interrupts::enable();

			// switch to another task
			core_scheduler.reschedule();

			// restore default values
			get_network_driver().unwrap().lock().set_polling_mode(true);
			backoff.reset();
		} else {
			// enable interrupts
			interrupts::enable();

			backoff.snooze();
		}
	}
}

/// Blocks the current thread on `f`, running the executor when idling.
pub(crate) fn poll_on<F, T>(future: F, timeout: Option<Duration>) -> Result<T, i32>
where
	F: Future<Output = Result<T, i32>>,
{
	// be sure that we are not interrupted by a timer, which is able
	// to call `reschedule`
	without_interrupts(|| {
		let start = now();
		let waker = core::task::Waker::noop();
		let mut cx = Context::from_waker(&waker);
		let mut future = future;
		let mut future = unsafe { core::pin::Pin::new_unchecked(&mut future) };

		loop {
			// run background tasks
			crate::executor::run();

			if let Poll::Ready(t) = future.as_mut().poll(&mut cx) {
				let wakeup_time = network_delay(now())
					.map(|d| crate::arch::processor::get_timer_ticks() + d.total_micros());
				core_scheduler().add_network_timer(wakeup_time);

				return t;
			}

			if let Some(duration) = timeout {
				if crate::executor::network::now() >= start + duration {
					let wakeup_time = network_delay(now())
						.map(|d| crate::arch::processor::get_timer_ticks() + d.total_micros());
					core_scheduler().add_network_timer(wakeup_time);

					return Err(-crate::errno::ETIME);
				}
			}
		}
	})
}
