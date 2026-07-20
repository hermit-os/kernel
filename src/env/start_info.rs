use core::num::NonZero;
use core::ptr;

use fdt::Fdt;
use hermit_entry::boot_info::{BootInfo, RawBootInfo};
use hermit_sync::OnceCell;

static BOOT_INFO: OnceCell<BootInfo> = OnceCell::new();

pub fn start_info() -> &'static (impl StartInfo + BootInfoExt) {
	BOOT_INFO.get().unwrap()
}

pub fn set_start_info(raw_boot_info: RawBootInfo) {
	let boot_info = BootInfo::from(raw_boot_info);
	BOOT_INFO.set(boot_info).unwrap();
}

pub trait StartInfo {
	fn bootargs(&self) -> Option<&str>;

	fn first_ram_address(&self) -> usize;

	#[cfg_attr(any(target_arch = "aarch64", target_arch = "riscv64"), expect(unused))]
	#[cfg(feature = "acpi")]
	fn rsdp_addr(&self) -> Option<NonZero<usize>>;
}

impl StartInfo for BootInfo {
	fn bootargs(&self) -> Option<&str> {
		self.fdt()?.chosen().bootargs()
	}

	fn first_ram_address(&self) -> usize {
		self.fdt()
			.unwrap()
			.memory()
			.regions()
			.next()
			.unwrap()
			.starting_address
			.expose_provenance()
	}

	#[cfg(feature = "acpi")]
	fn rsdp_addr(&self) -> Option<NonZero<usize>> {
		let rsdp = self
			.fdt()?
			.find_node("/hermit,rsdp")?
			.reg()?
			.next()?
			.starting_address
			.addr();
		NonZero::new(rsdp)
	}
}

pub trait BootInfoExt {
	fn fdt(&self) -> Option<Fdt<'_>>;

	fn fdt_addr(&self) -> Option<NonZero<usize>>;

	fn is_uefi(&self) -> bool;

	#[cfg(feature = "uhyve")]
	fn is_uhyve(&self) -> bool;

	#[cfg_attr(target_arch = "riscv64", expect(unused))]
	#[cfg(feature = "uhyve")]
	fn uhyve_boot_time(&self) -> Option<time::OffsetDateTime>;

	#[cfg_attr(any(target_arch = "aarch64", target_arch = "riscv64"), expect(unused))]
	#[cfg(all(feature = "uhyve", feature = "smp"))]
	fn uhyve_num_cpus(&self) -> Option<NonZero<usize>>;

	#[cfg_attr(any(target_arch = "aarch64", target_arch = "riscv64"), expect(unused))]
	#[cfg(feature = "uhyve")]
	fn uhyve_cpu_freq(&self) -> Option<NonZero<u32>>;
}

impl BootInfoExt for BootInfo {
	fn fdt(&self) -> Option<Fdt<'_>> {
		let fdt_addr = self.fdt_addr()?;
		let ptr = ptr::with_exposed_provenance(fdt_addr.get());
		let fdt = unsafe { Fdt::from_ptr(ptr).unwrap() };
		Some(fdt)
	}

	fn fdt_addr(&self) -> Option<NonZero<usize>> {
		let fdt_addr = self.hardware_info.device_tree?;
		let fdt_addr = NonZero::new(fdt_addr.get() as usize).unwrap();
		Some(fdt_addr)
	}

	fn is_uefi(&self) -> bool {
		let Some(fdt) = self.fdt() else {
			return false;
		};

		fdt.root().compatible().first() == "hermit,uefi"
	}

	#[cfg(feature = "uhyve")]
	fn is_uhyve(&self) -> bool {
		use hermit_entry::boot_info::PlatformInfo;

		matches!(self.platform_info, PlatformInfo::Uhyve { .. })
	}

	#[cfg(feature = "uhyve")]
	fn uhyve_boot_time(&self) -> Option<time::OffsetDateTime> {
		use hermit_entry::boot_info::PlatformInfo;

		match self.platform_info {
			PlatformInfo::Uhyve { boot_time, .. } => Some(boot_time),
			_ => None,
		}
	}

	#[cfg(all(feature = "uhyve", feature = "smp"))]
	fn uhyve_num_cpus(&self) -> Option<NonZero<usize>> {
		use hermit_entry::boot_info::PlatformInfo;

		match self.platform_info {
			PlatformInfo::Uhyve { num_cpus, .. } => {
				Some(NonZero::new(num_cpus.get() as usize).unwrap())
			}
			_ => None,
		}
	}

	#[cfg(feature = "uhyve")]
	fn uhyve_cpu_freq(&self) -> Option<NonZero<u32>> {
		use hermit_entry::boot_info::PlatformInfo;

		match self.platform_info {
			PlatformInfo::Uhyve { cpu_freq, .. } => Some(NonZero::new(cpu_freq?.get()).unwrap()),
			_ => None,
		}
	}
}
