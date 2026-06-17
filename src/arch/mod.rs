//! Architecture-specific architecture abstraction.

cfg_select! {
	target_arch = "aarch64" => {
		pub(crate) mod aarch64;
		pub(crate) use self::aarch64::*;
	}
	target_arch = "x86_64" => {
		pub(crate) mod x86_64;
		pub(crate) use self::x86_64::*;
	}
	target_arch = "riscv64" => {
		pub(crate) mod riscv64;
		pub(crate) use self::riscv64::*;
	}
}
