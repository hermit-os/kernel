#[cfg(feature = "hermit-entry")]
mod hermit_entry;

#[cfg(feature = "hermit-entry")]
pub use hermit_entry::*;

#[cfg(not(feature = "hermit-entry"))]
mod unsupported;

use core::fmt;
use core::num::NonZero;

#[cfg(not(feature = "hermit-entry"))]
pub use unsupported::*;

pub unsafe trait StartInfo {
	fn display(&self) -> impl fmt::Display {
		fmt::from_fn(|f| f.write_str("StartInfo::display not implemented"))
	}

	fn bootargs(&self) -> Option<&str> {
		None
	}

	#[cfg_attr(
		any(
			not(feature = "acpi"),
			target_arch = "aarch64",
			target_arch = "riscv64"
		),
		expect(dead_code)
	)]
	fn rsdp_addr(&self) -> Option<NonZero<usize>> {
		None
	}
}

#[cfg(any(
	feature = "hermit-entry",
	target_arch = "aarch64",
	target_arch = "riscv64"
))]
pub unsafe trait FdtStartInfo: StartInfo {
	fn fdt(&self) -> Option<fdt::Fdt<'_>> {
		None
	}

	#[cfg_attr(not(feature = "hermit-entry"), expect(dead_code))]
	fn fdt_addr(&self) -> Option<NonZero<usize>> {
		None
	}
}

#[cfg(feature = "uhyve")]
pub trait UhyveStartInfo: FdtStartInfo {
	fn is_uhyve(&self) -> bool;

	#[cfg_attr(target_arch = "riscv64", expect(unused))]
	fn uhyve_boot_time(&self) -> Option<time::OffsetDateTime>;

	#[cfg_attr(not(all(target_arch = "x86_64", feature = "smp")), expect(unused))]
	fn uhyve_num_cpus(&self) -> Option<NonZero<usize>>;
}
