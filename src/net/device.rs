use alloc::collections::BTreeMap;
use core::slice;

use smoltcp::iface::{InterfaceBuilder, NeighborCache, Routes};
#[cfg(feature = "trace")]
use smoltcp::phy::Tracer;
use smoltcp::phy::{self, Device, DeviceCapabilities};
#[cfg(feature = "dhcpv4")]
use smoltcp::socket::Dhcpv4Socket;
use smoltcp::time::Instant;
#[cfg(not(feature = "dhcpv4"))]
use smoltcp::wire::IpAddress;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpCidr, Ipv4Address};

use crate::net::{NetworkInterface, NetworkState};
use crate::syscalls::SYS;

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

/// Returns the value of the specified environment variable.
///
/// The value is fetched from the current runtime environment and, if not
/// present, falls back to the same environment variable set at compile time
/// (might not be present as well).
///
/// # Panics
///
/// Panics when environment variable is not valid unicode.
macro_rules! hermit_var {
	($name:expr) => {{
		use std::borrow::Cow;
		use std::env::{self, VarError};

		match env::var($name) {
			Ok(val) => Some(Cow::Owned(val)),
			Err(VarError::NotPresent) => option_env!($name).map(Cow::Borrowed),
			Err(VarError::NotUnicode(s)) => {
				panic!("couldn't interpret {}: {}", $name, VarError::NotUnicode(s))
			}
		}
	}};
}

/// Tries to parse the specified environment variable with a default value.
///
/// Parses according to [`hermit_var`] or returns the specified default value.
///
/// # Panics
///
/// Panics when environment variable is not valid unicode or cannot be parsed.
macro_rules! parse_hermit_var_or {
	($name:expr, $default:expr) => {{
		hermit_var!($name)
			.map(|ip| {
				ip.parse()
					.unwrap_or_else(|err| panic!("{err}: {}: {ip}", $name))
			})
			.unwrap_or($default)
	}};
}

impl NetworkInterface<HermitNet> {
	#[cfg(feature = "dhcpv4")]
	pub(crate) fn new() -> NetworkState {
		let mtu = match unsafe { SYS.get_mtu() } {
			Ok(mtu) => mtu,
			Err(_) => {
				return NetworkState::InitializationFailed;
			}
		};
		let device = HermitNet::new(mtu);
		#[cfg(feature = "trace")]
		let device = Tracer::new(device, |_timestamp, printer| {
			trace!("{}", printer);
		});

		let mac: [u8; 6] = match unsafe { SYS.get_mac_address() } {
			Ok(mac) => mac,
			Err(_) => {
				return NetworkState::InitializationFailed;
			}
		};

		let neighbor_cache = NeighborCache::new(BTreeMap::new());
		let ethernet_addr = EthernetAddress([mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]]);
		let hardware_addr = HardwareAddress::Ethernet(ethernet_addr);
		let ip_addrs = [IpCidr::new(Ipv4Address::UNSPECIFIED.into(), 0)];
		let routes = Routes::new(BTreeMap::new());

		info!("MAC address {}", hardware_addr);
		info!("MTU: {} bytes", mtu);

		let dhcp = Dhcpv4Socket::new();

		let mut iface = InterfaceBuilder::new(device, vec![])
			.hardware_addr(hardware_addr)
			.neighbor_cache(neighbor_cache)
			.ip_addrs(ip_addrs)
			.routes(routes)
			.finalize();

		let dhcp_handle = iface.add_socket(dhcp);

		NetworkState::Initialized(Self { iface, dhcp_handle })
	}

	#[cfg(not(feature = "dhcpv4"))]
	pub(crate) fn new() -> NetworkState {
		let mtu = match unsafe { SYS.get_mtu() } {
			Ok(mtu) => mtu,
			Err(_) => {
				return NetworkState::InitializationFailed;
			}
		};
		let device = HermitNet::new(mtu);
		#[cfg(feature = "trace")]
		let device = Tracer::new(device, |_timestamp, printer| {
			trace!("{}", printer);
		});

		let mac: [u8; 6] = match unsafe { SYS.get_mac_address() } {
			Ok(mac) => mac,
			Err(_) => {
				return NetworkState::InitializationFailed;
			}
		};

		let myip = parse_hermit_var_or!("HERMIT_IP", Ipv4Addr::new(10, 0, 5, 3));
		let myip = myip.octets();
		let mygw = parse_hermit_var_or!("HERMIT_GATEWAY", Ipv4Addr::new(10, 0, 5, 1));
		let mygw = mygw.octets();
		let mymask = parse_hermit_var_or!("HERMIT_MASK", Ipv4Addr::new(255, 255, 255, 0));
		let mymask = mymask.octets();

		// calculate the netmask length
		// => count the number of contiguous 1 bits,
		// starting at the most significant bit in the first octet
		let mut prefix_len = (!mymask[0]).trailing_zeros();
		if prefix_len == 8 {
			prefix_len += (!mymask[1]).trailing_zeros();
		}
		if prefix_len == 16 {
			prefix_len += (!mymask[2]).trailing_zeros();
		}
		if prefix_len == 24 {
			prefix_len += (!mymask[3]).trailing_zeros();
		}

		let neighbor_cache = NeighborCache::new(BTreeMap::new());
		let ethernet_addr = EthernetAddress([mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]]);
		let hardware_addr = HardwareAddress::Ethernet(ethernet_addr);
		let ip_addrs = [IpCidr::new(
			IpAddress::v4(myip[0], myip[1], myip[2], myip[3]),
			prefix_len.try_into().unwrap(),
		)];
		let default_v4_gw = Ipv4Address::new(mygw[0], mygw[1], mygw[2], mygw[3]);
		let routes = Routes::new(BTreeMap::new());
		routes.add_default_ipv4_route(default_v4_gw).unwrap();

		info!("MAC address {}", hardware_addr);
		info!("Configure network interface with address {}", ip_addrs[0]);
		info!("Configure gateway with address {}", default_v4_gw);
		info!("MTU: {} bytes", mtu);

		let iface = InterfaceBuilder::new(device, vec![])
			.hardware_addr(hardware_addr)
			.neighbor_cache(neighbor_cache)
			.ip_addrs(ip_addrs)
			.routes(routes)
			.finalize();

		NetworkState::Initialized(Self { iface })
	}
}

impl<'a> Device<'a> for HermitNet {
	type RxToken = RxToken;
	type TxToken = TxToken;

	fn capabilities(&self) -> DeviceCapabilities {
		let mut cap = DeviceCapabilities::default();
		cap.max_transmission_unit = self.mtu.into();
		cap
	}

	fn receive(&'a mut self) -> Option<(Self::RxToken, Self::TxToken)> {
		match unsafe { SYS.receive_rx_buffer() } {
			Ok((buffer, handle)) => Some((RxToken::new(buffer, handle), TxToken::new())),
			_ => None,
		}
	}

	fn transmit(&'a mut self) -> Option<Self::TxToken> {
		trace!("create TxToken to transfer data");
		Some(TxToken::new())
	}
}

#[doc(hidden)]
pub(crate) struct RxToken {
	buffer: &'static mut [u8],
	handle: usize,
}

impl RxToken {
	pub(crate) fn new(buffer: &'static mut [u8], handle: usize) -> Self {
		Self { buffer, handle }
	}
}

impl phy::RxToken for RxToken {
	#[allow(unused_mut)]
	fn consume<R, F>(mut self, _timestamp: Instant, f: F) -> smoltcp::Result<R>
	where
		F: FnOnce(&mut [u8]) -> smoltcp::Result<R>,
	{
		let result = f(self.buffer);
		if unsafe { SYS.rx_buffer_consumed(self.handle).is_ok() } {
			result
		} else {
			Err(smoltcp::Error::Exhausted)
		}
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
	fn consume<R, F>(self, _timestamp: Instant, len: usize, f: F) -> smoltcp::Result<R>
	where
		F: FnOnce(&mut [u8]) -> smoltcp::Result<R>,
	{
		let (tx_buffer, handle) = unsafe {
			SYS.get_tx_buffer(len)
				.map_err(|_| smoltcp::Error::Exhausted)?
		};
		let tx_slice: &'static mut [u8] = unsafe { slice::from_raw_parts_mut(tx_buffer, len) };
		match f(tx_slice) {
			Ok(result) => {
				if unsafe { SYS.send_tx_buffer(handle, len).is_ok() } {
					Ok(result)
				} else {
					Err(smoltcp::Error::Exhausted)
				}
			}
			Err(e) => {
				unsafe {
					let _ = 	SYS.free_tx_buffer(handle);
				}
				Err(e)
			}
		}
	}
}
