//! Architecture-specific architecture abstraction.

cfg_if::cfg_if! {
	if #[cfg(target_arch = "aarch64")] {
		pub(crate) mod aarch64;
		pub(crate) use self::aarch64::*;

		#[cfg(target_os = "none")]
		pub(crate) use self::aarch64::kernel::boot_processor_init;
		pub(crate) use self::aarch64::kernel::core_local;
		pub(crate) use self::aarch64::kernel::interrupts;
		pub(crate) use self::aarch64::kernel::interrupts::wakeup_core;
		#[cfg(feature = "pci")]
		pub(crate) use self::aarch64::kernel::pci;
		pub(crate) use self::aarch64::kernel::processor;
		pub(crate) use self::aarch64::kernel::processor::set_oneshot_timer;
		pub(crate) use self::aarch64::kernel::scheduler;
		pub(crate) use self::aarch64::kernel::switch;
		#[cfg(feature = "smp")]
		pub(crate) use self::aarch64::kernel::application_processor_init;
		pub(crate) use self::aarch64::kernel::{
			boot_application_processors,
			get_processor_count,
			message_output_init,
			output_message_buf,
		};
		pub use self::aarch64::mm::paging::{BasePageSize, PageSize};
	} else if #[cfg(target_arch = "x86_64")] {
		pub(crate) mod x86_64;
		pub(crate) use self::x86_64::*;

		pub(crate) use self::x86_64::kernel::apic::{
			set_oneshot_timer,
			wakeup_core,
		};
		#[cfg(all(target_os = "none", feature = "smp"))]
		pub(crate) use self::x86_64::kernel::application_processor_init;
		pub(crate) use self::x86_64::kernel::core_local;
		pub(crate) use self::x86_64::kernel::gdt::set_current_kernel_stack;
		pub(crate) use self::x86_64::kernel::interrupts;
		#[cfg(feature = "pci")]
		pub(crate) use self::x86_64::kernel::pci;
		pub(crate) use self::x86_64::kernel::processor;
		pub(crate) use self::x86_64::kernel::scheduler;
		pub(crate) use self::x86_64::kernel::switch;
		#[cfg(target_os = "none")]
		pub(crate) use self::x86_64::kernel::{
			boot_application_processors,
			boot_processor_init,
		};
		pub(crate) use self::x86_64::kernel::{
			get_processor_count,
			message_output_init,
			output_message_buf,
		};
		pub use self::x86_64::mm::paging::{BasePageSize, PageSize};
		#[cfg(feature = "common-os")]
		pub use self::x86_64::mm::create_new_root_page_table;
		#[cfg(feature = "common-os")]
		pub use self::x86_64::kernel::{load_application, jump_to_user_land};
	} else if #[cfg(target_arch = "riscv64")] {
		pub(crate) mod riscv64;
		pub(crate) use self::riscv64::*;

		#[cfg(feature = "smp")]
		pub(crate) use self::riscv64::kernel::application_processor_init;
		#[cfg(feature = "pci")]
		pub(crate) use self::riscv64::kernel::pci;
		pub(crate) use self::riscv64::kernel::processor::{self, set_oneshot_timer, wakeup_core};
		pub(crate) use self::riscv64::kernel::{
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
		pub use self::riscv64::mm::paging::{BasePageSize, PageSize};
	}
}
