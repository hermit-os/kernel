cfg_select! {
	feature = "hermit-entry" => {
		mod hermit_entry;
		pub use self::hermit_entry::*;
	}
	_ => {
		mod unsupported;
		pub use self::unsupported::*;
	}
}

mod memmap;
mod module;

use core::num::NonZero;
use core::{fmt, iter};

pub use self::memmap::{MemmapEntry, MemmapType};
pub use self::module::Module;

pub unsafe trait StartInfo {
	fn display(&self) -> impl fmt::Display {
		fmt::from_fn(|f| f.write_str("StartInfo::display not implemented"))
	}

	/// Returns the modules passed to the kernel at start.
	fn modules(&self) -> impl Iterator<Item = Module> {
		iter::empty()
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

	fn memmap(&self) -> impl Iterator<Item = MemmapEntry> {
		iter::empty()
	}
}

#[cfg(any(
	feature = "hermit-entry",
	target_arch = "aarch64",
	target_arch = "riscv64"
))]
pub unsafe trait FdtStartInfo: StartInfo {
	fn fdt(&self) -> Option<fdt::Fdt<'_>> {
		let phys_addr = self.fdt_addr()?.get();
		// We require this to be identity-mapped at boot time for now.
		let virt_addr = phys_addr;
		let ptr = core::ptr::with_exposed_provenance(virt_addr);
		let fdt = unsafe { fdt::Fdt::from_ptr(ptr).ok()? };
		Some(fdt)
	}

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
