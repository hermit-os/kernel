use core::ptr;

use fdt::Fdt;
use hermit_entry::boot_info::BootInfo;
use pvh::start_info::reader::{MemMap, StartInfoReader};

trait StartInfo {
	fn bootargs(&self) -> Option<&str>;
}

trait BootInfoExt {
	fn fdt(&self) -> Option<Fdt<'_>>;
}

impl BootInfoExt for BootInfo {
	fn fdt(&self) -> Option<Fdt<'_>> {
		self.hardware_info.device_tree.map(|fdt| {
			let ptr = ptr::with_exposed_provenance(fdt.get().try_into().unwrap());
			unsafe { Fdt::from_ptr(ptr).unwrap() }
		})
	}
}

impl StartInfo for BootInfo {
	fn bootargs(&self) -> Option<&str> {
		let fdt = self.fdt()?;
		fdt.chosen().bootargs()
	}
}

impl<'a, M: MemMap> StartInfo for StartInfoReader<'a, M> {
	fn bootargs(&self) -> Option<&str> {
		let cmdline = self.cmdline()?;

		match cmdline.to_str() {
			Ok(cmdline) => Some(cmdline),
			Err(err) => {
				error!("cmdline is not valid UTF-8: {err}");
				None
			}
		}
	}
}
