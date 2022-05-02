#[cfg(feature = "pci")]
pub mod rtl8139;
#[cfg(not(feature = "pci"))]
pub mod virtio_mmio;
pub mod virtio_net;
#[cfg(feature = "pci")]
pub mod virtio_pci;

use crate::arch::kernel::apic;
use crate::arch::kernel::irq::ExceptionStackFrame;
#[cfg(not(feature = "pci"))]
use crate::arch::kernel::mmio;
#[cfg(feature = "pci")]
use crate::arch::kernel::pci;
use crate::arch::kernel::percore::*;
#[cfg(all(not(feature = "newlib"), target_arch = "x86_64"))]
use crate::synch::semaphore::Semaphore;
use crate::synch::spinlock::SpinlockIrqSave;

/// A trait for accessing the network interface
pub trait NetworkInterface {
	/// Returns the mac address of the device.
	fn get_mac_address(&self) -> [u8; 6];
	/// Returns the current MTU of the device.
	fn get_mtu(&self) -> u16;
	/// Get buffer to create a TX packet
	///
	/// This returns ownership of the TX buffer.
	fn get_tx_buffer(&mut self, len: usize) -> Result<(*mut u8, usize), ()>;
	/// Frees the TX buffer (takes ownership)
	fn free_tx_buffer(&self, token: usize);
	/// Send TC packets (takes TX buffer ownership)
	fn send_tx_buffer(&mut self, tkn_handle: usize, len: usize) -> Result<(), ()>;
	/// Check if a packet is available
	fn has_packet(&self) -> bool;
	/// Get RX buffer with an received packet
	fn receive_rx_buffer(&mut self) -> Result<(&'static [u8], usize), ()>;
	/// Tells driver, that buffer is consumed and can be deallocated
	fn rx_buffer_consumed(&mut self, trf_handle: usize);
	/// Enable / disable the polling mode of the network interface
	fn set_polling_mode(&mut self, value: bool);
	/// Handle interrupt and check if a packet is available
	fn handle_interrupt(&mut self) -> bool;
}

#[cfg(all(not(feature = "newlib"), target_arch = "x86_64"))]
static NET_SEM: Semaphore = Semaphore::new(0);

/// set driver in polling mode and threads will not be blocked
pub extern "C" fn set_polling_mode(value: bool) {
	static THREADS_IN_POLLING_MODE: SpinlockIrqSave<usize> = SpinlockIrqSave::new(0);

	let mut guard = THREADS_IN_POLLING_MODE.lock();

	if value {
		*guard += 1;

		if *guard == 1 {
			#[cfg(feature = "pci")]
			if let Some(driver) = crate::arch::kernel::pci::get_network_driver() {
				driver.lock().set_polling_mode(value)
			}
		}
	} else {
		*guard -= 1;

		if *guard == 0 {
			#[cfg(feature = "pci")]
			if let Some(driver) = crate::arch::kernel::pci::get_network_driver() {
				driver.lock().set_polling_mode(value)
			}
		}
	}
}

#[cfg(all(not(feature = "newlib"), target_arch = "x86_64"))]
pub extern "C" fn netwait() {
	NET_SEM.acquire(None);
}

#[cfg(all(not(feature = "newlib"), target_arch = "x86_64"))]
pub fn netwakeup() {
	NET_SEM.release();
}

#[cfg(target_arch = "x86_64")]
pub extern "x86-interrupt" fn network_irqhandler(_stack_frame: ExceptionStackFrame) {
	debug!("Receive network interrupt");
	apic::eoi();

	#[cfg(feature = "pci")]
	let check_scheduler = match pci::get_network_driver() {
		Some(driver) => driver.lock().handle_interrupt(),
		_ => {
			debug!("Unable to handle interrupt!");
			false
		}
	};
	#[cfg(not(feature = "pci"))]
	let check_scheduler = match mmio::get_network_driver() {
		Some(driver) => driver.lock().handle_interrupt(),
		_ => {
			debug!("Unable to handle interrupt!");
			false
		}
	};

	if check_scheduler {
		core_scheduler().scheduler();
	}
}
