pub mod kernel;
pub mod mm;

/// Force strict CPU ordering, serializes load and store operations.
#[allow(dead_code)]
#[inline(always)]
pub(crate) fn memory_barrier() {
	use core::arch::asm;
	unsafe {
		asm!("mfence", options(nostack, nomem, preserves_flags),);
	}
}

pub fn init_drivers() {
	// Initialize PCI Drivers
	#[cfg(feature = "pci")]
	crate::drivers::pci::init_drivers();
	#[cfg(all(not(feature = "pci"), any(feature = "tcp", feature = "udp")))]
	crate::arch::x86_64::kernel::mmio::init_drivers();
}
