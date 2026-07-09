use core::num::NonZero;
use core::ptr;

use fdt::Fdt;
use hermit_entry::boot_info::{BootInfo, RawBootInfo};
use hermit_sync::OnceCell;
use memory_addresses::PhysAddr;

static BOOT_INFO: OnceCell<BootInfo> = OnceCell::new();

pub fn boot_info() -> &'static BootInfo {
	BOOT_INFO.get().unwrap()
}

pub fn set_boot_info(raw_boot_info: RawBootInfo) {
	let boot_info = BootInfo::from(raw_boot_info);
	BOOT_INFO.set(boot_info).unwrap();
}

/// Whether Hermit is running under the "uhyve" hypervisor.
#[cfg(feature = "uhyve")]
pub fn is_uhyve() -> bool {
	use hermit_entry::boot_info::PlatformInfo;

	matches!(boot_info().platform_info, PlatformInfo::Uhyve { .. })
}

#[cfg_attr(target_arch = "riscv64", expect(dead_code))]
#[cfg(feature = "uhyve")]
pub fn uhyve_boot_time() -> Option<time::OffsetDateTime> {
	use hermit_entry::boot_info::PlatformInfo;

	match boot_info().platform_info {
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

	match boot_info().platform_info {
		PlatformInfo::Uhyve { num_cpus, .. } => {
			Some(NonZero::new(num_cpus.get() as usize).unwrap())
		}
		_ => None,
	}
}

#[cfg_attr(not(target_arch = "x86_64"), expect(dead_code))]
#[cfg(feature = "uhyve")]
pub fn uhyve_cpu_freq() -> Option<NonZero<u32>> {
	use hermit_entry::boot_info::PlatformInfo;

	match boot_info().platform_info {
		PlatformInfo::Uhyve { cpu_freq, .. } => Some(NonZero::new(cpu_freq?.get()).unwrap()),
		_ => None,
	}
}

pub fn is_uefi() -> bool {
	fdt().is_some_and(|fdt| fdt.root().compatible().first() == "hermit,uefi")
}

pub fn fdt_addr() -> Option<NonZero<usize>> {
	boot_info()
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

pub(crate) fn get_ram_address() -> Option<PhysAddr> {
	let fdt = fdt()?;
	let memory = fdt.memory();
	let ptr = memory.regions().next()?.starting_address;
	Some(ptr.expose_provenance().into())
}

/// Returns the RSDP physical address if available.
#[cfg(all(target_arch = "x86_64", feature = "acpi"))]
pub fn rsdp() -> Option<NonZero<usize>> {
	let rsdp = fdt()?
		.find_node("/hermit,rsdp")?
		.reg()?
		.next()?
		.starting_address
		.addr();
	NonZero::new(rsdp)
}

pub fn fdt_args() -> Option<&'static str> {
	fdt().and_then(|fdt| fdt.chosen().bootargs())
}
