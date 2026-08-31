use alloc::vec::Vec;
use core::fmt;
use core::num::NonZero;

use fdt::Fdt;

use super::{MemmapEntry, MemmapType, Module, StartInfo};

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

unsafe impl<T: FdtStartInfo> StartInfo for T {
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

	fn modules(&self) -> impl Iterator<Item = Module> {
		fn initrd(fdt: Fdt<'_>) -> Option<Module> {
			let chosen = fdt.find_node("/chosen")?;

			let start = chosen.property("linux,initrd-start")?;
			let end = chosen.property("linux,initrd-end")?;

			let start = start.as_usize()?;
			let end = end.as_usize()?;
			let len = end.checked_sub(start)?;

			// SAFETY: The bootloader guarantees the addresses to be valid.
			let module = unsafe { Module::new(start, len) };

			Some(module)
		}

		self.fdt().and_then(initrd).into_iter()
	}

	fn memmap(&self) -> impl Iterator<Item = MemmapEntry> {
		// FIXME: use super let when available and don't collect
		let memmap = self
			.fdt()
			.iter()
			.flat_map(|fdt| fdt.find_all_nodes("/memory"))
			.flat_map(|node| node.reg())
			.flatten()
			.map(|region| MemmapEntry {
				phys_addr: region.starting_address.expose_provenance(),
				len: region.size.unwrap(),
				ty: MemmapType::Ram,
			})
			.collect::<Vec<_>>();
		memmap.into_iter()
	}
}
