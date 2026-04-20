use alloc::boxed::Box;
use core::str::FromStr;

use smoltcp::iface::{Config, Interface, SocketSet};
#[cfg(feature = "net-trace")]
use smoltcp::phy::Tracer;
use smoltcp::phy::{Device, Medium};
#[cfg(feature = "write-pcap-file")]
use smoltcp::phy::{PcapMode, PcapWriter};
#[cfg(feature = "dhcpv4")]
use smoltcp::socket::dhcpv4;
#[cfg(feature = "dns")]
use smoltcp::socket::dns;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpCidr, Ipv4Address, Ipv4Cidr};

use super::network::{NetworkInterface, NetworkState};
use crate::arch;
#[cfg(feature = "write-pcap-file")]
use crate::drivers::Driver;
use crate::drivers::net::NetworkDriver;

cfg_select! {
	any(
		all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci")),
		feature = "rtl8139",
		feature = "virtio-net",
	) => {
		use hermit_sync::SpinMutex;
		use crate::drivers::net::NetworkDevice;

		pub(crate) static NETWORK_DEVICE: SpinMutex<Option<NetworkDevice>> = SpinMutex::new(None);
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
			PcapWriter::new(
				device,
				pcap_writer::FileSink::new(default_name),
				PcapMode::Both,
			)
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

#[cfg(feature = "write-pcap-file")]
pub(in crate::executor) mod pcap_writer {
	use core::fmt::Write as _;

	use embedded_io::Write as _;
	use smoltcp::phy::PcapSink;

	use crate::errno::Errno;
	use crate::fs::File;

	/// Sink for packet captures. If the file Option is None, the writes are ignored.
	/// This is useful when we fail to create the sink file at runtime.
	pub struct FileSink(Option<File>);

	impl FileSink {
		pub(super) fn new(device_name: &str) -> Self {
			let (parent, file_prefix, extension) = parse_path(device_name);
			let mut path = format!("{parent}/{file_prefix}");
			let base_len = path.len();
			for i in 1.. {
				if let Some(extension) = extension {
					path.push('.');
					path.push_str(extension);
				}
				match File::create_new(path.as_str()) {
					Ok(file) => {
						info!("The packet capture will be written to '{path}'.");
						return Self(Some(file));
					}
					Err(Errno::Exist) => {
						path.truncate(base_len);
						write!(&mut path, " ({i})").unwrap();
					}
					Err(e) => {
						if e == Errno::Noent {
							error!("'{parent}/' does not exist. Is it mounted?");
						}
						error!(
							"Error {e:?} encountered while creating the pcap file. No pcap file will be written."
						);
						break;
					}
				}
			}
			Self(None)
		}
	}

	fn parse_path(device_name: &str) -> (&str, &str, Option<&str>) {
		let mut parent = "/root";
		let mut file_prefix = device_name;
		let mut extension = Some("pcap");

		if let Some(path) = crate::env::var("HERMIT_PCAP_PATH").filter(|var| !var.is_empty()) {
			let file_name = if let Some((l, r)) = path.rsplit_once('/') {
				parent = l;
				if r.is_empty() { None } else { Some(r) }
			} else {
				Some(path.as_str())
			};

			if let Some(file_name) = file_name {
				(file_prefix, extension) = if let Some((l, r)) = file_name.rsplit_once('.')
					&& !l.is_empty()
				{
					(l, Some(r))
				} else {
					(file_name, None)
				}
			};
		}
		(parent, file_prefix, extension)
	}

	impl PcapSink for FileSink {
		fn write(&mut self, data: &[u8]) {
			let Some(file) = self.0.as_mut() else {
				trace!("No file to write packet capture.");
				return;
			};

			if let Err(err) = file.write(data) {
				error!("Error while writing to the pcap file: {err}");
			}
		}

		fn flush(&mut self) {
			let Some(file) = self.0.as_mut() else {
				trace!("No file to write packet capture.");
				return;
			};

			if let Err(err) = file.flush() {
				error!("Error while flushing the pcap file: {err}");
			}
		}
	}
}
