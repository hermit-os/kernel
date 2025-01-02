#[cfg(all(target_arch = "riscv64", feature = "gem-net"))]
pub mod gem;
#[cfg(all(target_arch = "x86_64", feature = "rtl8139"))]
pub mod rtl8139;
#[cfg(not(all(target_arch = "x86_64", feature = "rtl8139")))]
pub mod virtio;

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
