//! Architecture-specific architecture abstraction.

cfg_select! {
	target_arch = "aarch64" => {
		pub(crate) mod aarch64;
		pub(crate) use self::aarch64::*;

		#[cfg(target_os = "none")]
		pub(crate) use self::aarch64::kernel::boot_processor_init;
		pub(crate) use self::aarch64::kernel::interrupts;
		pub(crate) use self::aarch64::kernel::interrupts::wakeup_core;
		pub(crate) use self::aarch64::kernel::processor;
		pub(crate) use self::aarch64::kernel::processor::set_oneshot_timer;
		pub(crate) use self::aarch64::kernel::scheduler;
		#[cfg(feature = "smp")]
		pub(crate) use self::aarch64::kernel::application_processor_init;
		pub(crate) use self::aarch64::kernel::{
			get_processor_count,
		};
		pub use self::aarch64::mm::paging::{BasePageSize, PageSize};
	}
	target_arch = "x86_64" => {
		pub(crate) mod x86_64;
		pub(crate) use self::x86_64::*;

		pub(crate) use self::x86_64::kernel::apic::{
			set_oneshot_timer,
			wakeup_core,
		};
		#[cfg(all(target_os = "none", feature = "smp"))]
		pub(crate) use self::x86_64::kernel::application_processor_init;
		pub(crate) use self::x86_64::kernel::interrupts;
		pub(crate) use self::x86_64::kernel::processor;
		pub(crate) use self::x86_64::kernel::scheduler;
		pub(crate) use self::x86_64::kernel::switch;
		#[cfg(target_os = "none")]
		pub(crate) use self::x86_64::kernel::boot_processor_init;
		pub(crate) use self::x86_64::kernel::{
			get_processor_count,
		};
		pub use self::x86_64::mm::paging::{BasePageSize, PageSize};
		#[cfg(feature = "common-os")]
		pub use self::x86_64::mm::create_new_root_page_table;
	}
	target_arch = "riscv64" => {
		pub(crate) mod riscv64;
		pub(crate) use self::riscv64::*;

		#[cfg(feature = "smp")]
		pub(crate) use self::riscv64::kernel::application_processor_init;
		pub(crate) use self::riscv64::kernel::processor::{self, set_oneshot_timer, wakeup_core};
		pub(crate) use self::riscv64::kernel::{
			boot_processor_init,
			get_processor_count,
			interrupts,
			scheduler,
			switch,
		};
		pub use self::riscv64::mm::paging::{BasePageSize, PageSize};
	}
}
