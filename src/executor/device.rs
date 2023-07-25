use alloc::boxed::Box;
use alloc::vec::Vec;
use core::slice;
#[cfg(not(feature = "dhcpv4"))]
use core::str::FromStr;

use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::phy::{self, Device, DeviceCapabilities, Medium};
#[cfg(feature = "dhcpv4")]
use smoltcp::socket::dhcpv4;
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress};
#[cfg(not(feature = "dhcpv4"))]
use smoltcp::wire::{IpAddress, IpCidr, Ipv4Address};

use super::network::{NetworkInterface, NetworkState};
use crate::arch;
#[cfg(not(feature = "pci"))]
use crate::arch::kernel::mmio as hardware;
#[cfg(feature = "pci")]
use crate::drivers::pci as hardware;

/// Data type to determine the mac address
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub(crate) struct HermitNet {
	pub mtu: u16,
}

impl HermitNet {
	pub(crate) const fn new(mtu: u16) -> Self {
		Self { mtu }
	}
}

impl<'a> NetworkInterface<'a> {
	#[cfg(feature = "dhcpv4")]
	pub(crate) fn create() -> NetworkState<'a> {
		let (mtu, mac) = if let Some(driver) = hardware::get_network_driver() {
			let guard = driver.lock();
			(guard.get_mtu(), guard.get_mac_address())
		} else {
			return NetworkState::InitializationFailed;
		};

		let mut device = HermitNet::new(mtu);

		let ethernet_addr = EthernetAddress([mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]]);
		let hardware_addr = HardwareAddress::Ethernet(ethernet_addr);

		info!("MAC address {}", hardware_addr);
		info!("MTU: {} bytes", mtu);

		let dhcp = dhcpv4::Socket::new();

		// use the current time based on the wall-clock time as seed
		let mut config = Config::new(hardware_addr);
		config.random_seed = (arch::get_boot_time() + arch::processor::get_timer_ticks()) / 1000000;
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
		}))
	}

	#[cfg(not(feature = "dhcpv4"))]
	pub(crate) fn create() -> NetworkState<'a> {
		let (mtu, mac) = if let Some(driver) = hardware::get_network_driver() {
			let guard = driver.lock();
			(guard.get_mtu(), guard.get_mac_address())
		} else {
			return NetworkState::InitializationFailed;
		};

		let mut device = HermitNet::new(mtu);

		let myip = Ipv4Address::from_str(hermit_var_or!("HERMIT_IP", "10.0.5.3")).unwrap();
		let mygw = Ipv4Address::from_str(hermit_var_or!("HERMIT_GATEWAY", "10.0.5.1")).unwrap();
		let mymask = Ipv4Address::from_str(hermit_var_or!("HERMIT_MASK", "255.255.255.0")).unwrap();

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
		config.random_seed = (arch::get_boot_time() + arch::processor::get_timer_ticks()) / 1000000;
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

		NetworkState::Initialized(Box::new(Self {
			iface,
			sockets: SocketSet::new(vec![]),
			device,
		}))
	}
}

impl Device for HermitNet {
	type RxToken<'a> = RxToken;
	type TxToken<'a> = TxToken;

	fn capabilities(&self) -> DeviceCapabilities {
		let mut cap = DeviceCapabilities::default();
		cap.max_transmission_unit = self.mtu.into();
		cap
	}

	fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
		if let Some(driver) = hardware::get_network_driver() {
			match driver.lock().receive_rx_buffer() {
				Ok(buffer) => Some((RxToken::new(buffer), TxToken::new())),
				_ => None,
			}
		} else {
			None
		}
	}

	fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
		trace!("create TxToken to transfer data");
		Some(TxToken::new())
	}
}

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
		let (tx_buffer, handle) = hardware::get_network_driver()
			.unwrap()
			.lock()
			.get_tx_buffer(len)
			.unwrap();
		let tx_slice: &'static mut [u8] = unsafe { slice::from_raw_parts_mut(tx_buffer, len) };
		let result = f(tx_slice);
		hardware::get_network_driver()
			.unwrap()
			.lock()
			.send_tx_buffer(handle, len)
			.expect("Unable to send TX buffer");
		result
	}
}
