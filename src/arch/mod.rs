// Platform-specific implementations
#[cfg(target_arch = "aarch64")]
pub mod aarch64;

#[cfg(target_arch = "x86_64")]
pub mod x86_64;

// Export our platform-specific modules.
#[cfg(all(target_arch = "aarch64", target_os = "none"))]
pub use crate::arch::aarch64::kernel::boot_processor_init;
#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::kernel::core_local;
#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::kernel::interrupts;
#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::kernel::interrupts::wakeup_core;
#[cfg(all(target_arch = "aarch64", feature = "pci"))]
pub use crate::arch::aarch64::kernel::pci;
#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::kernel::processor;
#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::kernel::processor::set_oneshot_timer;
#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::kernel::scheduler;
#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::kernel::switch;
#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::kernel::systemtime::get_boot_time;
#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::kernel::{
	application_processor_init, boot_application_processors, get_processor_count,
	message_output_init, output_message_buf,
};
#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::*;
#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::apic::{set_oneshot_timer, wakeup_core};
#[cfg(all(target_arch = "x86_64", target_os = "none", feature = "smp"))]
pub use crate::arch::x86_64::kernel::application_processor_init;
#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::core_local;
#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::gdt::set_current_kernel_stack;
#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::interrupts;
#[cfg(all(target_arch = "x86_64", feature = "pci"))]
pub use crate::arch::x86_64::kernel::pci;
#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::processor;
#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::scheduler;
#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::switch;
#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::systemtime::get_boot_time;
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub use crate::arch::x86_64::kernel::{boot_application_processors, boot_processor_init};
#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::kernel::{
	get_processor_count, message_output_init, output_message_buf,
};
#[cfg(target_arch = "x86_64")]
pub use crate::arch::x86_64::*;

pub fn init_drivers() {
	// Initialize PCI Drivers for x86_64
	#[cfg(feature = "pci")]
	crate::drivers::pci::init_drivers();
	#[cfg(all(target_arch = "x86_64", not(feature = "pci"), feature = "tcp"))]
	crate::arch::x86_64::kernel::mmio::init_drivers();
}
