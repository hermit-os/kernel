use core::num::NonZero;

use hermit_entry::boot_info::{BootInfo, RawBootInfo};
use hermit_sync::OnceCell;

use super::{FdtStartInfo, StartInfo};

static START_INFO: OnceCell<BootInfo> = OnceCell::new();

#[cfg(not(feature = "uhyve"))]
pub fn start_info() -> &'static (impl StartInfo + FdtStartInfo) {
	START_INFO.get().unwrap()
}

#[cfg(feature = "uhyve")]
pub fn start_info() -> &'static (impl StartInfo + super::UhyveStartInfo) {
	START_INFO.get().unwrap()
}

pub unsafe fn set_start_info(raw_boot_info: RawBootInfo) {
	let start_info = BootInfo::from(raw_boot_info);
	START_INFO.set(start_info).unwrap();
}

unsafe impl FdtStartInfo for BootInfo {
	fn fdt_addr(&self) -> Option<NonZero<usize>> {
		let fdt_addr = self.hardware_info.device_tree?;
		let fdt_addr = NonZero::new(fdt_addr.get() as usize).unwrap();
		Some(fdt_addr)
	}
}

#[cfg(feature = "uhyve")]
impl super::UhyveStartInfo for BootInfo {
	fn is_uhyve(&self) -> bool {
		let Some(fdt) = self.fdt() else {
			return false;
		};

		fdt.root()
			.compatible()
			.all()
			.any(|compatible| compatible == "hermit,uhyve")
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
