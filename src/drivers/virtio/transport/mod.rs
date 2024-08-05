//! A module containing virtios transport mechanisms.
//!
//! The module contains only PCI specific transport mechanism.
//! Other mechanisms (MMIO and Channel I/O) are currently not
//! supported.

#[cfg(not(feature = "pci"))]
pub mod mmio;
#[cfg(feature = "pci")]
pub mod pci;

use hermit_sync::OnceCell;

#[cfg(not(target_arch = "riscv64"))]
use crate::arch::kernel::core_local::increment_irq_counter;
#[cfg(target_arch = "x86_64")]
use crate::arch::kernel::interrupts::ExceptionStackFrame;
#[cfg(all(feature = "vsock", not(feature = "pci")))]
use crate::arch::kernel::mmio as hardware;
#[cfg(target_arch = "aarch64")]
use crate::arch::scheduler::State;
#[cfg(any(feature = "tcp", feature = "udp"))]
use crate::drivers::net::NetworkDriver;
#[cfg(all(feature = "vsock", feature = "pci"))]
use crate::drivers::pci as hardware;

/// All virtio devices share the interrupt number `VIRTIO_IRQ`
static VIRTIO_IRQ: OnceCell<u8> = OnceCell::new();

#[cfg(target_arch = "aarch64")]
pub(crate) fn virtio_irqhandler(_state: &State) -> bool {
	debug!("Receive virtio interrupt");

	crate::executor::run();

	#[cfg(any(feature = "tcp", feature = "udp"))]
	if let Some(driver) = hardware::get_network_driver() {
		driver.lock().handle_interrupt()
	}

	#[cfg(feature = "vsock")]
	if let Some(driver) = hardware::get_vsock_driver() {
		driver.lock().handle_interrupt();
	}
}

#[cfg(target_arch = "x86_64")]
pub(crate) extern "x86-interrupt" fn virtio_irqhandler(stack_frame: ExceptionStackFrame) {
	crate::arch::x86_64::swapgs(&stack_frame);
	use crate::arch::kernel::core_local::core_scheduler;
	use crate::scheduler::PerCoreSchedulerExt;

	debug!("Receive virtio interrupt");

	increment_irq_counter(32 + VIRTIO_IRQ.get().unwrap());

	crate::executor::run();
	crate::kernel::apic::eoi();

	#[cfg(any(feature = "tcp", feature = "udp"))]
	if let Some(driver) = hardware::get_network_driver() {
		driver.lock().handle_interrupt()
	}

	#[cfg(feature = "vsock")]
	if let Some(driver) = hardware::get_vsock_driver() {
		driver.lock().handle_interrupt();
	}

	core_scheduler().reschedule();
	crate::arch::x86_64::swapgs(&stack_frame);
}

#[cfg(target_arch = "riscv64")]
pub(crate) fn virtio_irqhandler() {
	use crate::arch::kernel::core_local::core_scheduler;
	use crate::scheduler::PerCoreSchedulerExt;

	debug!("Receive virtio interrupt");

	increment_irq_counter(32 + VIRTIO_IRQ.get().unwrap());

	crate::executor::run();

	// PLIC end of interrupt
	crate::arch::kernel::interrupts::external_eoi();
	#[cfg(any(feature = "tcp", feature = "udp"))]
	if let Some(driver) = hardware::get_network_driver() {
		driver.lock().handle_interrupt()
	}

	#[cfg(feature = "vsock")]
	if let Some(driver) = hardware::get_vsock_driver() {
		driver.lock().handle_interrupt();
	}

	core_scheduler().reschedule();
}
