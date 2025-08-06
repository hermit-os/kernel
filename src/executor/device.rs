use alloc::boxed::Box;
use alloc::vec::Vec;
#[cfg(not(feature = "dhcpv4"))]
use core::str::FromStr;

use cfg_if::cfg_if;
use smoltcp::iface::{Config, Interface, SocketSet};
#[cfg(feature = "trace")]
use smoltcp::phy::Tracer;
use smoltcp::phy::{self, ChecksumCapabilities, Device, DeviceCapabilities, Medium};
#[cfg(feature = "dhcpv4")]
use smoltcp::socket::dhcpv4;
#[cfg(all(feature = "dns", not(feature = "dhcpv4")))]
use smoltcp::socket::dns;
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress};
#[cfg(not(feature = "dhcpv4"))]
use smoltcp::wire::{IpCidr, Ipv4Address, Ipv4Cidr};

use super::network::{NetworkInterface, NetworkState};
use crate::arch;
use crate::drivers::net::{NetworkDevice, NetworkDriver};
use crate::mm::device_alloc::DeviceAlloc;

cfg_if! {
	if #[cfg(any(
		all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci")),
		all(target_arch = "x86_64", feature = "rtl8139"),
		feature = "virtio-net",
	))] {
		use hermit_sync::SpinMutex;

		pub(crate) static NETWORK_DEVICE: SpinMutex<Option<NetworkDevice>> = SpinMutex::new(Option::None);
	} else {
		use crate::drivers::net::loopback::LoopbackDriver;
	}
}

/// Data type to determine the mac address
#[repr(C)]
pub(crate) struct HermitNet {
	mtu: u16,
	checksums: ChecksumCapabilities,
	device: NetworkDevice,
}

impl HermitNet {
	pub(crate) const fn new(
		mtu: u16,
		checksums: ChecksumCapabilities,
		device: NetworkDevice,
	) -> Self {
		Self {
			mtu,
			checksums,
			device,
		}
	}

	#[cfg(any(
		all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci")),
		all(target_arch = "x86_64", feature = "rtl8139"),
		feature = "virtio-net",
	))]
	pub(crate) fn handle_interrupt(&mut self) {
		self.device.handle_interrupt();
	}

	pub(crate) fn set_polling_mode(&mut self, value: bool) {
		self.device.set_polling_mode(value);
	}
}

impl<'a> NetworkInterface<'a> {
	#[cfg(feature = "dhcpv4")]
	pub(crate) fn create() -> NetworkState<'a> {
		cfg_if! {
			if #[cfg(any(
				all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci")),
				all(target_arch = "x86_64", feature = "rtl8139"),
				feature = "virtio-net",
			))] {
				let Some(device) = NETWORK_DEVICE.lock().take() else {
					return NetworkState::InitializationFailed;
				};
			} else {
				let device = LoopbackDriver::new();
			}
		}

		let (mtu, mac, checksums) = (
			device.get_mtu(),
			device.get_mac_address(),
			device.get_checksums(),
		);

		let mut device = {
			let device = HermitNet::new(mtu, checksums.clone(), device);
			#[cfg(feature = "trace")]
			let device = Tracer::new(device, |timestamp, printer| trace!("{timestamp} {printer}"));
			device
		};

		if hermit_var!("HERMIT_IP").is_some() {
			warn!(
				"A static IP address is specified with the environment variable HERMIT_IP, but the device is configured to use DHCPv4!"
			);
		}

		let ethernet_addr = EthernetAddress([mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]]);
		let hardware_addr = HardwareAddress::Ethernet(ethernet_addr);

		info!("MAC address {hardware_addr}");
		info!("{checksums:?}");
		info!("MTU: {mtu} bytes");

		let dhcp = dhcpv4::Socket::new();

		// use the current time based on the wall-clock time as seed
		let mut config = Config::new(hardware_addr);
		config.random_seed = (arch::kernel::systemtime::now_micros()) / 1_000_000;
		if device.capabilities().medium == Medium::Ethernet {
			config.hardware_addr = hardware_addr;
		}

		let iface = Interface::new(config, &mut device, crate::executor::network::now());
		let mut sockets = SocketSet::new(vec![]);
		let dhcp_handle = sockets.add(dhcp);

		NetworkState::Initialized(Box::new(Self {
			iface,
			sockets,
			device,
			dhcp_handle,
			#[cfg(feature = "dns")]
			dns_handle: None,
		}))
	}

	#[cfg(not(feature = "dhcpv4"))]
	pub(crate) fn create() -> NetworkState<'a> {
		cfg_if! {
			if #[cfg(any(
				all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci")),
				all(target_arch = "x86_64", feature = "rtl8139"),
				feature = "virtio-net",
			))] {
				let Some(device) = NETWORK_DEVICE.lock().take() else {
					return NetworkState::InitializationFailed;
				};
			} else {
				let device = LoopbackDriver::new();
			}
		}

		let (mtu, mac, checksums) = (
			device.get_mtu(),
			device.get_mac_address(),
			device.get_checksums(),
		);

		let mut device = {
			let device = HermitNet::new(mtu, checksums.clone(), device);
			#[cfg(feature = "trace")]
			let device = Tracer::new(device, |timestamp, printer| trace!("{timestamp} {printer}"));
			device
		};

		let myip = Ipv4Address::from_str(hermit_var_or!("HERMIT_IP", "10.0.5.3")).unwrap();
		let mygw = Ipv4Address::from_str(hermit_var_or!("HERMIT_GATEWAY", "10.0.5.1")).unwrap();
		let mymask = Ipv4Address::from_str(hermit_var_or!("HERMIT_MASK", "255.255.255.0")).unwrap();
		// Quad9 DNS server
		#[cfg(feature = "dns")]
		let mydns1 = Ipv4Address::from_str(hermit_var_or!("HERMIT_DNS1", "9.9.9.9")).unwrap();
		// Cloudflare DNS server
		#[cfg(feature = "dns")]
		let mydns2 = Ipv4Address::from_str(hermit_var_or!("HERMIT_DNS2", "1.1.1.1")).unwrap();

		let ethernet_addr = EthernetAddress(mac);
		let hardware_addr = HardwareAddress::Ethernet(ethernet_addr);
		let ip_addr = IpCidr::from(Ipv4Cidr::from_netmask(myip, mymask).unwrap());

		info!("MAC address {hardware_addr}");
		info!("Configure network interface with address {ip_addr}");
		info!("Configure gateway with address {mygw}");
		info!("{checksums:?}");
		info!("MTU: {mtu} bytes");

		// use the current time based on the wall-clock time as seed
		let mut config = Config::new(hardware_addr);
		config.random_seed = (arch::kernel::systemtime::now_micros()) / 1_000_000;
		if device.capabilities().medium == Medium::Ethernet {
			config.hardware_addr = hardware_addr;
		}

		let mut iface = Interface::new(config, &mut device, crate::executor::network::now());
		iface.update_ip_addrs(|ip_addrs| {
			ip_addrs.push(ip_addr).unwrap();
		});
		iface.routes_mut().add_default_ipv4_route(mygw).unwrap();

		#[allow(unused_mut)]
		let mut sockets = SocketSet::new(vec![]);

		#[cfg(feature = "dns")]
		let dns_handle = {
			let servers = &[mydns1.into(), mydns2.into()];
			let dns_socket = dns::Socket::new(servers, vec![]);
			sockets.add(dns_socket)
		};

		NetworkState::Initialized(Box::new(Self {
			iface,
			sockets,
			device,
			#[cfg(feature = "dns")]
			dns_handle: Some(dns_handle),
		}))
	}
}

impl Device for HermitNet {
	type RxToken<'a> = RxToken;
	type TxToken<'a> = TxToken<'a>;

	fn capabilities(&self) -> DeviceCapabilities {
		let mut cap = DeviceCapabilities::default();
		cap.max_transmission_unit = self.mtu.into();
		cap.max_burst_size = Some(0x10000 / cap.max_transmission_unit);
		cap.checksum = self.checksums.clone();
		cap
	}

	fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
		self.device.receive_packet()
	}

	fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
		Some(TxToken::new(&mut self.device))
	}
}

#[doc(hidden)]
pub(crate) struct RxToken {
	buffer: Vec<u8, DeviceAlloc>,
}

impl RxToken {
	pub(crate) fn new(buffer: Vec<u8, DeviceAlloc>) -> Self {
		Self { buffer }
	}
}

impl phy::RxToken for RxToken {
	fn consume<R, F>(self, f: F) -> R
	where
		F: FnOnce(&[u8]) -> R,
	{
		f(&self.buffer[..])
	}
}

#[doc(hidden)]
pub(crate) struct TxToken<'a> {
	device: &'a mut NetworkDevice,
}

impl<'a> TxToken<'a> {
	pub(crate) fn new(device: &'a mut NetworkDevice) -> Self {
		Self { device }
	}
}

impl<'a> phy::TxToken for TxToken<'a> {
	fn consume<R, F>(self, len: usize, f: F) -> R
	where
		F: FnOnce(&mut [u8]) -> R,
	{
		self.device.send_packet(len, f)
	}
}
