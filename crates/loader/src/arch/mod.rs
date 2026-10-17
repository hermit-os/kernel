cfg_if::cfg_if! {
	if #[cfg(target_arch = "aarch64")] {
		mod aarch64;
		pub use self::aarch64::*;
	} else if #[cfg(target_arch = "riscv64")] {
		mod riscv64;
		pub use self::riscv64::*;
	} else if #[cfg(all(target_arch = "x86_64"))] {
		mod x86_64;
		pub use self::x86_64::*;
	}
}
