use core::num::NonZero;
use core::ptr;

use fdt::Fdt;
use hermit_entry::boot_info::{BootInfo, RawBootInfo};
use hermit_sync::OnceCell;

static START_INFO: OnceCell<BootInfo> = OnceCell::new();

pub fn start_info() -> &'static BootInfo {
	START_INFO.get().unwrap()
}

pub fn set_start_info(raw_boot_info: RawBootInfo) {
	let start_info = BootInfo::from(raw_boot_info);
	START_INFO.set(start_info).unwrap();
}

/// Whether Hermit is running under the "uhyve" hypervisor.
#[cfg(feature = "uhyve")]
pub fn is_uhyve() -> bool {
	use hermit_entry::boot_info::PlatformInfo;

	matches!(start_info().platform_info, PlatformInfo::Uhyve { .. })
}

#[cfg_attr(target_arch = "riscv64", expect(dead_code))]
#[cfg(feature = "uhyve")]
pub fn uhyve_boot_time() -> Option<time::OffsetDateTime> {
	use hermit_entry::boot_info::PlatformInfo;

	match start_info().platform_info {
		PlatformInfo::Uhyve { boot_time, .. } => Some(boot_time),
		_ => None,
	}
}

#[cfg_attr(
	any(not(target_arch = "x86_64"), not(feature = "smp")),
	expect(dead_code)
)]
#[cfg(feature = "uhyve")]
pub fn uhyve_num_cpus() -> Option<NonZero<usize>> {
	use hermit_entry::boot_info::PlatformInfo;

	match start_info().platform_info {
		PlatformInfo::Uhyve { num_cpus, .. } => {
			Some(NonZero::new(num_cpus.get() as usize).unwrap())
		}
		_ => None,
	}
}

pub fn fdt_addr() -> Option<NonZero<usize>> {
	start_info()
		.hardware_info
		.device_tree
		.map(|fdt| NonZero::new(fdt.get() as usize).unwrap())
}

pub fn fdt() -> Option<Fdt<'static>> {
	fdt_addr().map(|fdt| {
		let ptr = ptr::with_exposed_provenance(fdt.get());
		unsafe { Fdt::from_ptr(ptr).unwrap() }
	})
}

/// Returns the RSDP physical address if available.
#[cfg_attr(
	any(
		not(feature = "acpi"),
		target_arch = "aarch64",
		target_arch = "riscv64"
	),
	expect(dead_code)
)]
pub fn rsdp_addr() -> Option<NonZero<usize>> {
	let rsdp_addr = fdt()?
		.find_node("/hermit,rsdp")?
		.reg()?
		.next()?
		.starting_address
		.addr();
	NonZero::new(rsdp_addr)
}

pub fn bootargs() -> Option<&'static str> {
	fdt().and_then(|fdt| fdt.chosen().bootargs())
}
