use core::num::NonZero;

use fdt::Fdt;

pub unsafe trait FdtStartInfo {
	fn fdt(&self) -> Option<Fdt<'_>> {
		let phys_addr = self.fdt_addr()?.get();
		// We require this to be identity-mapped at boot time for now.
		let virt_addr = phys_addr;
		let ptr = core::ptr::with_exposed_provenance(virt_addr);
		let fdt = unsafe { Fdt::from_ptr(ptr).ok()? };
		Some(fdt)
	}

	fn fdt_addr(&self) -> Option<NonZero<usize>> {
		None
	}
}
