#[cfg(feature = "rtl8139")]
pub mod rtl8139;
#[cfg(all(not(feature = "pci"), not(feature = "rtl8139")))]
pub mod virtio_mmio;
#[cfg(not(feature = "rtl8139"))]
pub mod virtio_net;
#[cfg(all(feature = "pci", not(feature = "rtl8139")))]
pub mod virtio_pci;

#[cfg(target_arch = "x86_64")]
use crate::arch::kernel::apic;
#[allow(unused_imports)]
use crate::arch::kernel::core_local::*;
#[cfg(target_arch = "x86_64")]
use crate::arch::kernel::interrupts::ExceptionStackFrame;
#[cfg(not(feature = "pci"))]
use crate::arch::kernel::mmio as hardware;
#[cfg(target_arch = "aarch64")]
use crate::arch::scheduler::State;
#[cfg(feature = "pci")]
use crate::drivers::pci as hardware;
use crate::executor::device::{RxToken, TxToken};

/// A trait for accessing the network interface
pub(crate) trait NetworkDriver {
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
	fn has_packet(&self) -> bool;
	/// Enable / disable the polling mode of the network interface
	fn set_polling_mode(&mut self, value: bool);
	/// Handle interrupt and check if a packet is available
	fn handle_interrupt(&mut self) -> bool;
	/// Returns true, if the device has to create checksums
	fn with_checksums(&self) -> bool {
		true
	}
}

#[inline]
fn _irqhandler() -> bool {
	let result = if let Some(driver) = hardware::get_network_driver() {
		driver.lock().handle_interrupt()
	} else {
		debug!("Unable to handle interrupt!");
		false
	};

	// TODO: do we need it?
	crate::executor::run();

	result
}

#[cfg(target_arch = "aarch64")]
pub(crate) fn network_irqhandler(_state: &State) -> bool {
	debug!("Receive network interrupt");
	_irqhandler()
}

#[cfg(target_arch = "x86_64")]
pub(crate) extern "x86-interrupt" fn network_irqhandler(_stack_frame: ExceptionStackFrame) {
	debug!("Receive network interrupt");
	apic::eoi();
	let _ = _irqhandler();

	core_scheduler().reschedule();
}
