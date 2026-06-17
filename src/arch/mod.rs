//! Architecture-specific architecture abstraction.

cfg_select! {
	target_arch = "aarch64" => {
		pub(crate) mod aarch64;
		pub(crate) use self::aarch64::*;

		pub(crate) use self::aarch64::kernel::interrupts::wakeup_core;
		pub(crate) use self::aarch64::kernel::processor::set_oneshot_timer;
	}
	target_arch = "x86_64" => {
		pub(crate) mod x86_64;
		pub(crate) use self::x86_64::*;

		pub(crate) use self::x86_64::kernel::apic::{
			set_oneshot_timer,
			wakeup_core,
		};
		pub(crate) use self::x86_64::kernel::switch;
		#[cfg(feature = "common-os")]
		pub use self::x86_64::mm::create_new_root_page_table;
	}
	target_arch = "riscv64" => {
		pub(crate) mod riscv64;
		pub(crate) use self::riscv64::*;

		pub(crate) use self::riscv64::kernel::processor::{set_oneshot_timer, wakeup_core};
		pub(crate) use self::riscv64::kernel::{
			switch,
		};
	}
}
