pub mod kernel;
pub mod mm;

#[allow(dead_code)]
#[cfg(target_arch = "riscv64")]
#[inline(always)]
pub(crate) fn memory_barrier() {
	riscv::asm::sfence_vma_all()
}

pub fn init_drivers() {
	// Initialize PCI Drivers
	#[cfg(feature = "pci")]
	crate::drivers::pci::init_drivers();
}
