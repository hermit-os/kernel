//! Architecture-specific architecture abstraction.

cfg_if::cfg_if! {
	if #[cfg(target_arch = "aarch64")] {
		pub mod aarch64;
		pub use self::aarch64::*;

		#[cfg(target_os = "none")]
		pub use self::aarch64::kernel::boot_processor_init;
		pub use self::aarch64::kernel::core_local;
		pub use self::aarch64::kernel::interrupts;
		pub use self::aarch64::kernel::interrupts::wakeup_core;
		#[cfg(feature = "pci")]
		pub use self::aarch64::kernel::pci;
		pub use self::aarch64::kernel::processor;
		pub use self::aarch64::kernel::processor::set_oneshot_timer;
		pub use self::aarch64::kernel::scheduler;
		pub use self::aarch64::kernel::switch;
		pub use self::aarch64::kernel::systemtime::get_boot_time;
		#[cfg(feature = "smp")]
		pub use self::aarch64::kernel::application_processor_init;
		pub use self::aarch64::kernel::{
			boot_application_processors,
			get_processor_count,
			message_output_init,
			output_message_buf,
		};
	} else if #[cfg(target_arch = "x86_64")] {
		pub mod x86_64;
		pub use self::x86_64::*;

		pub use self::x86_64::kernel::apic::{
			set_oneshot_timer,
			wakeup_core,
		};
		#[cfg(all(target_os = "none", feature = "smp"))]
		pub use self::x86_64::kernel::application_processor_init;
		pub use self::x86_64::kernel::core_local;
		pub use self::x86_64::kernel::gdt::set_current_kernel_stack;
		pub use self::x86_64::kernel::interrupts;
		#[cfg(feature = "pci")]
		pub use self::x86_64::kernel::pci;
		pub use self::x86_64::kernel::processor;
		pub use self::x86_64::kernel::scheduler;
		pub use self::x86_64::kernel::switch;
		pub use self::x86_64::kernel::systemtime::get_boot_time;
		#[cfg(target_os = "none")]
		pub use self::x86_64::kernel::{
			boot_application_processors,
			boot_processor_init,
		};
		pub use self::x86_64::kernel::{
			get_processor_count,
			message_output_init,
			output_message_buf,
		};
	} else if #[cfg(target_arch = "riscv64")] {
		pub mod riscv64;
		pub use self::riscv64::*;

		#[cfg(feature = "smp")]
		pub use self::riscv64::kernel::application_processor_init;
		pub use self::riscv64::kernel::processor::{self, set_oneshot_timer, wakeup_core};
		pub use self::riscv64::kernel::systemtime::get_boot_time;
		pub use self::riscv64::kernel::{
			boot_application_processors,
			boot_processor_init,
			core_local,
			get_processor_count,
			interrupts,
			message_output_init,
			output_message_buf,
			scheduler,
			switch,
		};
	}
}
