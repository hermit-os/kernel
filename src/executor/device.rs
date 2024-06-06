use alloc::boxed::Box;
use alloc::vec::Vec;
#[cfg(not(feature = "dhcpv4"))]
use core::str::FromStr;

use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::phy::{self, ChecksumCapabilities, Device, DeviceCapabilities, Medium};
#[cfg(feature = "dhcpv4")]
use smoltcp::socket::dhcpv4;
#[cfg(all(feature = "dns", not(feature = "dhcpv4")))]
use smoltcp::socket::dns;
use smoltcp::time::Instant;
#[cfg(not(feature = "dhcpv4"))]
use smoltcp::wire::Ipv4Address;
use smoltcp::wire::{EthernetAddress, HardwareAddress};
#[cfg(not(feature = "dhcpv4"))]
use smoltcp::wire::{IpAddress, IpCidr};

use super::network::{NetworkInterface, NetworkState};
use crate::arch;
#[cfg(not(feature = "pci"))]
use crate::arch::kernel::mmio as hardware;
use crate::drivers::net::NetworkDriver;
#[cfg(feature = "pci")]
use crate::drivers::pci as hardware;

/// Data type to determine the mac address
#[derive(Debug, Clone)]
#[repr(C)]
pub(crate) struct HermitNet {
	mtu: u16,
	checksums: ChecksumCapabilities,
}

impl HermitNet {
	pub(crate) const fn new(mtu: u16, checksums: ChecksumCapabilities) -> Self {
		Self { mtu, checksums }
	}
}

impl<'a> NetworkInterface<'a> {
	#[cfg(feature = "dhcpv4")]
	pub(crate) fn create() -> NetworkState<'a> {
		let (mtu, mac, checksums) = if let Some(driver) = hardware::get_network_driver() {
			let guard = driver.lock();
			(
				guard.get_mtu(),
				guard.get_mac_address(),
				guard.get_checksums(),
			)
		} else {
			return NetworkState::InitializationFailed;
		};

		let mut device = HermitNet::new(mtu, checksums);

		if hermit_var!("HERMIT_IP").is_some() {
			warn!("A static IP address is specified with the environment variable HERMIT_IP, but the device is configured to use DHCPv4!");
		}

		let ethernet_addr = EthernetAddress([mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]]);
		let hardware_addr = HardwareAddress::Ethernet(ethernet_addr);

		info!("MAC address {}", hardware_addr);
		info!("MTU: {} bytes", mtu);

		let dhcp = dhcpv4::Socket::new();

		// use the current time based on the wall-clock time as seed
		let mut config = Config::new(hardware_addr);
		config.random_seed = (arch::kernel::systemtime::now_micros()) / 1000000;
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
		let (mtu, mac, checksums) = if let Some(driver) = hardware::get_network_driver() {
			let guard = driver.lock();
			(
				guard.get_mtu(),
				guard.get_mac_address(),
				guard.get_checksums(),
			)
		} else {
			return NetworkState::InitializationFailed;
		};

		let mut device = HermitNet::new(mtu, checksums);

		let myip = Ipv4Address::from_str(hermit_var_or!("HERMIT_IP", "10.0.5.3")).unwrap();
		let mygw = Ipv4Address::from_str(hermit_var_or!("HERMIT_GATEWAY", "10.0.5.1")).unwrap();
		let mymask = Ipv4Address::from_str(hermit_var_or!("HERMIT_MASK", "255.255.255.0")).unwrap();
		// Quad9 DNS server
		#[cfg(feature = "dns")]
		let mydns1 = Ipv4Address::from_str(hermit_var_or!("HERMIT_DNS1", "9.9.9.9")).unwrap();
		// Cloudflare DNS server
		#[cfg(feature = "dns")]
		let mydns2 = Ipv4Address::from_str(hermit_var_or!("HERMIT_DNS2", "1.1.1.1")).unwrap();

		// calculate the netmask length
		// => count the number of contiguous 1 bits,
		// starting at the most significant bit in the first octet
		let mut prefix_len = (!mymask.as_bytes()[0]).trailing_zeros();
		if prefix_len == 8 {
			prefix_len += (!mymask.as_bytes()[1]).trailing_zeros();
		}
		if prefix_len == 16 {
			prefix_len += (!mymask.as_bytes()[2]).trailing_zeros();
		}
		if prefix_len == 24 {
			prefix_len += (!mymask.as_bytes()[3]).trailing_zeros();
		}

		let ethernet_addr = EthernetAddress([mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]]);
		let hardware_addr = HardwareAddress::Ethernet(ethernet_addr);
		let ip_addrs = [IpCidr::new(
			IpAddress::v4(
				myip.as_bytes()[0],
				myip.as_bytes()[1],
				myip.as_bytes()[2],
				myip.as_bytes()[3],
			),
			prefix_len.try_into().unwrap(),
		)];

		info!("MAC address {}", hardware_addr);
		info!("Configure network interface with address {}", ip_addrs[0]);
		info!("Configure gateway with address {}", mygw);
		info!("MTU: {} bytes", mtu);

		// use the current time based on the wall-clock time as seed
		let mut config = Config::new(hardware_addr);
		config.random_seed = (arch::kernel::systemtime::now_micros()) / 1000000;
		if device.capabilities().medium == Medium::Ethernet {
			config.hardware_addr = hardware_addr;
		}

		let mut iface = Interface::new(config, &mut device, crate::executor::network::now());
		iface.update_ip_addrs(|ip_addrs| {
			ip_addrs
				.push(IpCidr::new(
					IpAddress::v4(
						myip.as_bytes()[0],
						myip.as_bytes()[1],
						myip.as_bytes()[2],
						myip.as_bytes()[3],
					),
					prefix_len.try_into().unwrap(),
				))
				.unwrap();
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
	type TxToken<'a> = TxToken;

	fn capabilities(&self) -> DeviceCapabilities {
		let mut cap = DeviceCapabilities::default();
		cap.max_transmission_unit = self.mtu.into();
		cap.max_burst_size = Some(65535 / cap.max_transmission_unit);
		cap.checksum = self.checksums.clone();
		cap
	}

	fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
		if let Some(driver) = hardware::get_network_driver() {
			driver.lock().receive_packet()
		} else {
			None
		}
	}

	fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
		Some(TxToken::new())
	}
}

// Unique handle to identify the RxToken
pub(crate) type RxHandle = usize;

#[doc(hidden)]
pub(crate) struct RxToken {
	buffer: Vec<u8>,
}

impl RxToken {
	pub(crate) fn new(buffer: Vec<u8>) -> Self {
		Self { buffer }
	}
}

impl phy::RxToken for RxToken {
	fn consume<R, F>(mut self, f: F) -> R
	where
		F: FnOnce(&mut [u8]) -> R,
	{
		f(&mut self.buffer[..])
	}
}

#[doc(hidden)]
pub(crate) struct TxToken;

impl TxToken {
	pub(crate) fn new() -> Self {
		Self {}
	}
}

impl phy::TxToken for TxToken {
	fn consume<R, F>(self, len: usize, f: F) -> R
	where
		F: FnOnce(&mut [u8]) -> R,
	{
		hardware::get_network_driver()
			.unwrap()
			.lock()
			.send_packet(len, f)
	}
}
