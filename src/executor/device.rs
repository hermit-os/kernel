use alloc::boxed::Box;
#[cfg(not(feature = "dhcpv4"))]
use core::str::FromStr;

use cfg_if::cfg_if;
use smoltcp::iface::{Config, Interface, SocketSet};
#[cfg(feature = "trace")]
use smoltcp::phy::Tracer;
use smoltcp::phy::{Device, Medium};
#[cfg(feature = "dhcpv4")]
use smoltcp::socket::dhcpv4;
#[cfg(all(feature = "dns", not(feature = "dhcpv4")))]
use smoltcp::socket::dns;
use smoltcp::wire::{EthernetAddress, HardwareAddress};
#[cfg(not(feature = "dhcpv4"))]
use smoltcp::wire::{IpCidr, Ipv4Address, Ipv4Cidr};

use super::network::{NetworkInterface, NetworkState};
use crate::arch;
use crate::drivers::net::NetworkDriver;

cfg_if! {
	if #[cfg(any(
		all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci")),
		all(target_arch = "x86_64", feature = "rtl8139"),
		feature = "virtio-net",
	))] {
		use hermit_sync::SpinMutex;
		use crate::drivers::net::NetworkDevice;

		pub(crate) static NETWORK_DEVICE: SpinMutex<Option<NetworkDevice>> = SpinMutex::new(Option::None);
	} else {
		use crate::drivers::net::loopback::LoopbackDriver;
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
				#[cfg_attr(feature = "trace", expect(unused_mut))]
				let Some(mut device) = NETWORK_DEVICE.lock().take() else {
					return NetworkState::InitializationFailed;
				};
			} else {
				#[cfg_attr(feature = "trace", expect(unused_mut))]
				let mut device = LoopbackDriver::new();
			}
		}

		let mac = device.get_mac_address();

		#[cfg(feature = "trace")]
		let mut device = Tracer::new(device, |timestamp, printer| trace!("{timestamp} {printer}"));

		if let Some(hermit_ip) = hermit_var!("HERMIT_IP") {
			warn!("HERMIT_IP was set to {hermit_ip}, but Hermit was built with DHCPv4.");
			warn!("Ignoring HERMIT_IP.");
		}

		let ethernet_addr = EthernetAddress([mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]]);
		let hardware_addr = HardwareAddress::Ethernet(ethernet_addr);

		info!("MAC address: {hardware_addr}");
		let capabilities = device.capabilities();
		info!("{:?}", capabilities.checksum);
		info!("MTU: {} bytes", capabilities.max_transmission_unit);

		let dhcp = dhcpv4::Socket::new();

		// use the current time based on the wall-clock time as seed
		let mut config = Config::new(hardware_addr);
		config.random_seed = (arch::kernel::systemtime::now_micros()) / 1_000_000;
		if capabilities.medium == Medium::Ethernet {
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
				#[cfg_attr(feature = "trace", expect(unused_mut))]
				let Some(mut device) = NETWORK_DEVICE.lock().take() else {
					return NetworkState::InitializationFailed;
				};
			} else {
				#[cfg_attr(feature = "trace", expect(unused_mut))]
				let mut device = LoopbackDriver::new();
			}
		}

		let mac = device.get_mac_address();

		#[cfg(feature = "trace")]
		let mut device = Tracer::new(device, |timestamp, printer| trace!("{timestamp} {printer}"));

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

		info!("MAC address: {hardware_addr}");
		let capabilities = device.capabilities();
		info!("{:?}", capabilities.checksum);
		info!("MTU: {} bytes", capabilities.max_transmission_unit);
		info!("IP address: {ip_addr}");
		info!("Gateway:    {mygw}");

		// use the current time based on the wall-clock time as seed
		let mut config = Config::new(hardware_addr);
		config.random_seed = (arch::kernel::systemtime::now_micros()) / 1_000_000;
		if capabilities.medium == Medium::Ethernet {
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
