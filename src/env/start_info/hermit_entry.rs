use core::num::NonZero;
use core::{fmt, ptr};

use fdt::Fdt;
use hermit_entry::boot_info::{BootInfo, RawBootInfo};
use hermit_sync::OnceCell;

use super::{FdtStartInfo, StartInfo};

static START_INFO: OnceCell<BootInfo> = OnceCell::new();

#[cfg(not(feature = "uhyve"))]
pub fn start_info() -> &'static impl FdtStartInfo {
	START_INFO.get().unwrap()
}

#[cfg(feature = "uhyve")]
pub fn start_info() -> &'static impl super::UhyveStartInfo {
	START_INFO.get().unwrap()
}

pub fn set_start_info(raw_boot_info: RawBootInfo) {
	let start_info = BootInfo::from(raw_boot_info);
	START_INFO.set(start_info).unwrap();
}

impl StartInfo for BootInfo {
	fn display(&self) -> impl fmt::Display {
		fmt::from_fn(|f| {
			if let Some(fdt) = self.fdt() {
				write!(f, "FDT:\n{fdt:#?}")
			} else {
				f.write_str("No FDT.")
			}
		})
	}

	fn bootargs(&self) -> Option<&str> {
		self.fdt()?.chosen().bootargs()
	}

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

impl FdtStartInfo for BootInfo {
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
}

#[cfg(feature = "uhyve")]
impl super::UhyveStartInfo for BootInfo {
	fn is_uhyve(&self) -> bool {
		use hermit_entry::boot_info::PlatformInfo;

		matches!(self.platform_info, PlatformInfo::Uhyve { .. })
	}

	fn uhyve_boot_time(&self) -> Option<time::OffsetDateTime> {
		use hermit_entry::boot_info::PlatformInfo;

		match self.platform_info {
			PlatformInfo::Uhyve { boot_time, .. } => Some(boot_time),
			_ => None,
		}
	}

	fn uhyve_num_cpus(&self) -> Option<NonZero<usize>> {
		use hermit_entry::boot_info::PlatformInfo;

		match self.platform_info {
			PlatformInfo::Uhyve { num_cpus, .. } => {
				Some(NonZero::new(num_cpus.get() as usize).unwrap())
			}
			_ => None,
		}
	}
}
