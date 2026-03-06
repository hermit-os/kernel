use alloc::boxed::Box;
#[cfg(not(feature = "dhcpv4"))]
use core::str::FromStr;

use smoltcp::iface::{Config, Interface, SocketSet};
#[cfg(feature = "net-trace")]
use smoltcp::phy::Tracer;
use smoltcp::phy::{Device, Medium};
#[cfg(feature = "dhcpv4")]
use smoltcp::socket::dhcpv4;
#[cfg(all(feature = "dns", not(feature = "dhcpv4")))]
use smoltcp::socket::dns;
use smoltcp::wire::{EthernetAddress, HardwareAddress};
#[cfg(not(feature = "dhcpv4"))]
use smoltcp::wire::{IpCidr, Ipv4Address, Ipv4Cidr};
#[cfg(feature = "write-pcap-file")]
use {
	crate::drivers::Driver,
	crate::fs::File,
	embedded_io::Write,
	smoltcp::phy::{PcapMode, PcapSink, PcapWriter},
};

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
				#[cfg_attr(any(feature = "net-trace", feature = "write-pcap-file"), expect(unused_mut))]
				let Some(mut device) = NETWORK_DEVICE.lock().take() else {
					return NetworkState::InitializationFailed;
				};
			}
			_ => {
				#[cfg_attr(any(feature = "net-trace", feature = "write-pcap-file"), expect(unused_mut))]
				let mut device = LoopbackDriver::new();
			}
		}

		let mac = device.get_mac_address();

		#[cfg_attr(feature = "net-trace", expect(unused_mut))]
		#[cfg(feature = "write-pcap-file")]
		let mut device = {
			let default_name = device.get_name();
			PcapWriter::new(device, FileSink::new(default_name), PcapMode::Both)
		};

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

		#[cfg_attr(feature = "dhcpv4", expect(unused_mut))]
		let mut iface = Interface::new(config, &mut device, crate::executor::network::now());
		#[cfg_attr(all(not(feature = "dhcpv4"), not(feature = "dns")), expect(unused_mut))]
		let mut sockets = SocketSet::new(vec![]);

		cfg_select! {
			feature = "dhcpv4" => {
				if let Some(hermit_ip) = hermit_var!("HERMIT_IP") {
					warn!("HERMIT_IP was set to {hermit_ip}, but Hermit was built with DHCPv4.");
					warn!("Ignoring HERMIT_IP.");
				}

				let dhcp = dhcpv4::Socket::new();
				let dhcp_handle = sockets.add(dhcp);
				#[cfg(feature = "dns")]
				let dns_handle = None;
			}
			_ => {
				let myip = Ipv4Address::from_str(hermit_var_or!("HERMIT_IP", "10.0.5.3")).unwrap();
				let mygw =
					Ipv4Address::from_str(hermit_var_or!("HERMIT_GATEWAY", "10.0.5.1")).unwrap();
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
				let dns_handle = {
					// Quad9 DNS server
					let mydns1 =
						Ipv4Address::from_str(hermit_var_or!("HERMIT_DNS1", "9.9.9.9")).unwrap();
					// Cloudflare DNS server
					let mydns2 =
						Ipv4Address::from_str(hermit_var_or!("HERMIT_DNS2", "1.1.1.1")).unwrap();
					let servers = &[mydns1.into(), mydns2.into()];
					let dns_socket = dns::Socket::new(servers, vec![]);
					Some(sockets.add(dns_socket))
				};
			}
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

#[cfg(feature = "write-pcap-file")]
/// Sink for packet captures. If the file Option is None, the writes are ignored.
/// This is useful when we fail to create the sink file at runtime.
pub(in crate::executor) struct FileSink(Option<File>);

#[cfg(feature = "write-pcap-file")]
impl FileSink {
	fn new(default_name: &str) -> Self {
		use core::ops::ControlFlow;

		use crate::errno::Errno;

		let file_name_base = option_env!("HERMIT_PCAP_NAME").unwrap_or(default_name);
		let mut file_name = format!("{file_name_base}.pcap");
		let file = (1..)
			.try_for_each(|i| {
				let path = format!("/root/{file_name}");
				match File::create_new(path.as_str()) {
					Err(Errno::Exist) => {
						file_name = format!("{file_name_base} ({i}).pcap");
						ControlFlow::Continue(())
					}
					r => ControlFlow::Break(r),
				}
			})
			.break_value()
			.unwrap();

		if let Err(e) = file {
			if e == Errno::Noent {
				error!("/root is not mounted. Are there any mount points for the VM?");
			}
			error!(
				"Error {e:?} encountered while creating the pcap file. No pcap file will be written."
			);
		} else {
			info!(
				"The packet capture will be written to a file called \"{file_name}\" under the mount point."
			);
		}

		FileSink(file.ok())
	}
}

#[cfg(feature = "write-pcap-file")]
impl PcapSink for FileSink {
	fn write(&mut self, data: &[u8]) {
		if let Some(file) = self.0.as_mut()
			&& let Some(err) = file.write(data).err()
		{
			error!("Error while writing to the pcap file: {err}");
		}
	}

	fn flush(&mut self) {
		if let Some(file) = self.0.as_mut()
			&& let Some(err) = file.flush().err()
		{
			error!("Error while flushing the pcap file: {err}");
		}
	}
}
