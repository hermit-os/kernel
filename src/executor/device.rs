use alloc::boxed::Box;
use core::str::FromStr;

use smoltcp::iface::{Config, Interface, SocketSet};
#[cfg(feature = "net-trace")]
use smoltcp::phy::Tracer;
use smoltcp::phy::{Device, Medium};
#[cfg(feature = "dhcpv4")]
use smoltcp::socket::dhcpv4;
#[cfg(feature = "dns")]
use smoltcp::socket::dns;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpCidr, Ipv4Address, Ipv4Cidr};

use super::network::{NetworkInterface, NetworkState};
use crate::arch;
use crate::drivers::net::NetworkDriver;

cfg_select! {
	any(
		all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci")),
		feature = "rtl8139",
		feature = "virtio-net",
	) => {
		use hermit_sync::SpinMutex;
		use crate::drivers::net::NetworkDevice;

		pub(crate) static NETWORK_DEVICE: SpinMutex<Option<NetworkDevice>> = SpinMutex::new(Option::None);
	}
	_ => {
		use crate::drivers::net::loopback::LoopbackDriver;
	}
}

impl<'a> NetworkInterface<'a> {
	pub(crate) fn create() -> NetworkState<'a> {
		cfg_select! {
			any(
				all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci")),
				feature = "rtl8139",
				feature = "virtio-net",
			) => {
				#[cfg_attr(feature = "net-trace", expect(unused_mut))]
				let Some(mut device) = NETWORK_DEVICE.lock().take() else {
					return NetworkState::InitializationFailed;
				};
			}
			_ => {
				#[cfg_attr(feature = "net-trace", expect(unused_mut))]
				let mut device = LoopbackDriver::new();
			}
		}

		let mac = device.get_mac_address();

		#[cfg(feature = "net-trace")]
		let mut device = Tracer::new(device, |timestamp, printer| trace!("{timestamp} {printer}"));

		let ethernet_addr = EthernetAddress(mac);
		let hardware_addr = HardwareAddress::Ethernet(ethernet_addr);

		info!("MAC address: {hardware_addr}");
		let capabilities = device.capabilities();
		info!("{:?}", capabilities.checksum);
		info!("MTU: {} bytes", capabilities.max_transmission_unit);

		// use the current time based on the wall-clock time as seed
		let mut config = Config::new(hardware_addr);
		config.random_seed = (arch::kernel::systemtime::now_micros()) / 1_000_000;
		if capabilities.medium == Medium::Ethernet {
			config.hardware_addr = hardware_addr;
		}

		let mut iface = Interface::new(config, &mut device, crate::executor::network::now());
		#[cfg_attr(all(not(feature = "dhcpv4"), not(feature = "dns")), expect(unused_mut))]
		let mut sockets = SocketSet::new(vec![]);

		#[cfg(feature = "dns")]
		let mut dns_handle = None;

		#[cfg(feature = "dhcpv4")]
		let dhcp_handle = {
			if let Some(hermit_ip) = hermit_var!("HERMIT_IP") {
				warn!("HERMIT_IP was set to {hermit_ip}, but Hermit was built with DHCPv4.");
				warn!(
					"HERMIT_IP will be overwritten if a DHCP configuration is acquired. If the provided configuration was not meant to be a fallback, disable the DHCP feature."
				);
			}
			sockets.add(dhcpv4::Socket::new())
		};

		if !cfg!(feature = "dhcpv4") || hermit_var!("HERMIT_IP").is_some() {
			let myip = Ipv4Address::from_str(hermit_var_or!("HERMIT_IP", "10.0.5.3")).unwrap();
			let mygw = Ipv4Address::from_str(hermit_var_or!("HERMIT_GATEWAY", "10.0.5.1")).unwrap();
			let mymask =
				Ipv4Address::from_str(hermit_var_or!("HERMIT_MASK", "255.255.255.0")).unwrap();

			let ip_addr = IpCidr::from(Ipv4Cidr::from_netmask(myip, mymask).unwrap());
			info!("IP address: {ip_addr}");
			info!("Gateway:    {mygw}");

			iface.update_ip_addrs(|ip_addrs| {
				ip_addrs.push(ip_addr).unwrap();
			});
			iface.routes_mut().add_default_ipv4_route(mygw).unwrap();

			#[cfg(feature = "dns")]
			{
				// Quad9 DNS server
				let mydns1 =
					Ipv4Address::from_str(hermit_var_or!("HERMIT_DNS1", "9.9.9.9")).unwrap();
				// Cloudflare DNS server
				let mydns2 =
					Ipv4Address::from_str(hermit_var_or!("HERMIT_DNS2", "1.1.1.1")).unwrap();
				let servers = &[mydns1.into(), mydns2.into()];
				let dns_socket = dns::Socket::new(servers, vec![]);
				dns_handle = Some(sockets.add(dns_socket));
			};
		}

		NetworkState::Initialized(Box::new(Self {
			iface,
			sockets,
			device,
			#[cfg(feature = "dhcpv4")]
			dhcp_handle,
			#[cfg(feature = "dns")]
			dns_handle,
		}))
	}
}
