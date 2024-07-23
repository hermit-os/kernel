//! A module containing virtios transport mechanisms.
//!
//! The module contains only PCI specific transport mechanism.
//! Other mechanisms (MMIO and Channel I/O) are currently not
//! supported.

#[cfg(not(feature = "pci"))]
pub mod mmio;
#[cfg(feature = "pci")]
pub mod pci;

#[cfg(target_arch = "x86_64")]
use crate::arch::kernel::interrupts::ExceptionStackFrame;
#[cfg(target_arch = "aarch64")]
use crate::arch::scheduler::State;
#[cfg(any(feature = "tcp", feature = "udp"))]
use crate::drivers::net::network_irqhandler;

#[cfg(target_arch = "aarch64")]
pub(crate) fn virtio_irqhandler(_state: &State) -> bool {
	debug!("Receive virtio interrupt");
	cfg_if::cfg_if! {
		if #[cfg(any(feature = "tcp", feature = "udp"))] {
			network_irqhandler()
		} else {
			false
		}
	}
}

#[cfg(target_arch = "x86_64")]
pub(crate) extern "x86-interrupt" fn virtio_irqhandler(stack_frame: ExceptionStackFrame) {
	crate::arch::x86_64::swapgs(&stack_frame);
	use crate::arch::kernel::core_local::core_scheduler;
	use crate::scheduler::PerCoreSchedulerExt;

	info!("Receive virtio interrupt");
	crate::kernel::apic::eoi();
	#[cfg(any(feature = "tcp", feature = "udp"))]
	let _ = network_irqhandler();

	core_scheduler().reschedule();
	crate::arch::x86_64::swapgs(&stack_frame);
}

#[cfg(target_arch = "riscv64")]
pub(crate) fn virtio_irqhandler() {
	use crate::arch::kernel::core_local::core_scheduler;
	use crate::scheduler::PerCoreSchedulerExt;

	debug!("Receive virtio interrupt");

	// PLIC end of interrupt
	crate::arch::kernel::interrupts::external_eoi();
	#[cfg(any(feature = "tcp", feature = "udp"))]
	let _ = network_irqhandler();

	core_scheduler().reschedule();
}
