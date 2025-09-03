#[cfg(all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci")))]
pub mod gem;
#[cfg(not(any(
	all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci")),
	all(target_arch = "x86_64", feature = "rtl8139"),
	feature = "virtio-net",
)))]
pub mod loopback;
#[cfg(all(target_arch = "x86_64", feature = "rtl8139"))]
pub mod rtl8139;
#[cfg(all(
	not(all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci"))),
	not(all(target_arch = "x86_64", feature = "rtl8139")),
	feature = "virtio-net",
))]
pub mod virtio;
#[allow(unused_imports)]
use crate::arch::kernel::core_local::*;
use crate::drivers::Driver;

/// A trait for accessing the network interface
pub(crate) trait NetworkDriver: Driver + smoltcp::phy::Device {
	/// Returns the mac address of the device.
	fn get_mac_address(&self) -> [u8; 6];
	/// Check if a packet is available
	#[allow(dead_code)]
	fn has_packet(&self) -> bool;
	/// Enable / disable the polling mode of the network interface
	fn set_polling_mode(&mut self, value: bool);
	/// Handle interrupt and check if a packet is available
	#[allow(dead_code)]
	fn handle_interrupt(&mut self);
}

/// Determines the MTU that should be used as configured by crate features
/// or environment variables.
#[cfg(any(
	all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci")),
	all(target_arch = "x86_64", feature = "rtl8139"),
	feature = "virtio-net",
))]
pub(crate) fn mtu() -> u16 {
	use core::str::FromStr;

	// Default IP level MTU to use.
	const DEFAULT_IP_MTU: u16 = 1500;

	/// Default MTU to use.
	///
	/// This is 1500 IP MTU and a 14-byte ethernet header.
	const DEFAULT_MTU: u16 = DEFAULT_IP_MTU + 14;

	if let Some(my_mtu) = hermit_var!("HERMIT_MTU") {
		u16::from_str(&my_mtu).unwrap()
	} else {
		DEFAULT_MTU
	}
}

cfg_if::cfg_if! {
	if #[cfg(all(
		not(feature = "pci"),
		any(
			all(target_arch = "riscv64", feature = "gem-net"),
			feature = "virtio-net",
		),
	))] {
		pub(crate) use crate::arch::kernel::mmio::NetworkDevice;
	} else if #[cfg(all(
		feature = "pci",
		any(
			all(target_arch = "x86_64", feature = "rtl8139"),
			feature = "virtio-net",
		),
	))] {
		pub(crate) use crate::drivers::pci::NetworkDevice;
	} else {
		pub(crate) use loopback::NetworkDevice;
	}
}
