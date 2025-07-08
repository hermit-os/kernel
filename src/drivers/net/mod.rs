#[cfg(all(target_arch = "riscv64", feature = "gem-net", not(feature = "pci")))]
pub mod gem;
#[cfg(all(target_arch = "x86_64", feature = "rtl8139"))]
pub mod rtl8139;
#[cfg(feature = "virtio-net")]
pub mod virtio;

use core::str::FromStr;

use smoltcp::phy::ChecksumCapabilities;

#[allow(unused_imports)]
use crate::arch::kernel::core_local::*;
use crate::drivers::Driver;
use crate::executor::device::{RxToken, TxToken};

/// A trait for accessing the network interface
pub(crate) trait NetworkDriver: Driver {
	/// Returns smoltcp's checksum capabilities
	fn get_checksums(&self) -> ChecksumCapabilities {
		ChecksumCapabilities::default()
	}
	/// Returns the mac address of the device.
	fn get_mac_address(&self) -> [u8; 6];
	/// Returns the current MTU of the device.
	fn get_mtu(&self) -> u16;
	/// Get buffer with the received packet
	fn receive_packet(&mut self) -> Option<(RxToken, TxToken)>;
	/// Send packet with the size `len`
	fn send_packet<R, F>(&mut self, len: usize, f: F) -> R
	where
		F: FnOnce(&mut [u8]) -> R;
	/// Check if a packet is available
	#[allow(dead_code)]
	fn has_packet(&self) -> bool;
	/// Enable / disable the polling mode of the network interface
	fn set_polling_mode(&mut self, value: bool);
	/// Handle interrupt and check if a packet is available
	fn handle_interrupt(&mut self);
}

// Default IP level MTU to use.
const DEFAULT_IP_MTU: u16 = 1500;

/// Default MTU to use.
///
/// This is 1500 IP MTU and a 14-byte ethernet header.
const DEFAULT_MTU: u16 = DEFAULT_IP_MTU + 14;

/// Determines the MTU that should be used as configured by crate features
/// or environment variables.
pub(crate) fn mtu() -> u16 {
	if let Some(my_mtu) = hermit_var!("HERMIT_MTU") {
		u16::from_str(&my_mtu).unwrap()
	} else {
		DEFAULT_MTU
	}
}
