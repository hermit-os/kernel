use alloc::format;
use alloc::vec::Vec;
use core::ops::Range;

use vm_fdt::{FdtWriter, FdtWriterNode, FdtWriterResult};

pub struct Fdt<'a> {
	writer: FdtWriter,
	root_node: FdtWriterNode,
	bootargs: Option<&'a str>,
}

impl<'a> Fdt<'a> {
	pub fn new(platform: &str) -> FdtWriterResult<Self> {
		let mut writer = FdtWriter::new()?;

		let root_node = writer.begin_node("")?;
		writer.property_string("compatible", &format!("hermit,{platform}"))?;
		writer.property_u32("#address-cells", 0x2)?;
		writer.property_u32("#size-cells", 0x2)?;

		let bootargs = None;

		Ok(Self {
			writer,
			root_node,
			bootargs,
		})
	}

	pub fn finish(mut self) -> FdtWriterResult<Vec<u8>> {
		let chosen_node = self.writer.begin_node("chosen")?;
		if let Some(bootargs) = self.bootargs {
			self.writer.property_string("bootargs", bootargs)?;
		}
		self.writer.end_node(chosen_node)?;

		self.writer.end_node(self.root_node)?;

		self.writer.finish()
	}

	#[cfg_attr(target_os = "uefi", expect(unused))]
	pub fn bootargs(mut self, bootargs: &'a str) -> FdtWriterResult<Self> {
		assert!(self.bootargs.is_none());
		self.bootargs = Some(bootargs);

		Ok(self)
	}

	#[cfg_attr(all(target_arch = "x86_64", not(target_os = "uefi")), expect(unused))]
	pub fn rsdp(mut self, rsdp: u64) -> FdtWriterResult<Self> {
		let rsdp_node = self.writer.begin_node(&format!("hermit,rsdp@{rsdp:x}"))?;
		self.writer.property_array_u64("reg", &[rsdp, 1])?;
		self.writer.end_node(rsdp_node)?;

		Ok(self)
	}

	pub fn memory(mut self, memory: Range<u64>) -> FdtWriterResult<Self> {
		let memory_node = self
			.writer
			.begin_node(format!("memory@{:x}", memory.start).as_str())?;
		self.writer.property_string("device_type", "memory")?;
		self.writer
			.property_array_u64("reg", &[memory.start, memory.end - memory.start])?;
		self.writer.end_node(memory_node)?;

		Ok(self)
	}
}

#[cfg(all(target_arch = "x86_64", not(target_os = "uefi"), not(feature = "fc")))]
mod x86_64 {
	use multiboot::information::{MemoryMapIter, MemoryType};
	use vm_fdt::FdtWriterResult;

	impl super::Fdt<'_> {
		pub fn memory_regions(
			mut self,
			memory_regions: MemoryMapIter<'_, '_>,
		) -> FdtWriterResult<Self> {
			let memory_regions =
				memory_regions.filter(|m| m.memory_type() == MemoryType::Available);

			for memory_region in memory_regions {
				self = self.memory(
					memory_region.base_address()
						..memory_region.base_address() + memory_region.length(),
				)?;
			}

			Ok(self)
		}
	}
}

#[cfg(target_os = "uefi")]
mod uefi {
	use core::fmt;
	use core::fmt::Write;

	use log::info;
	use uefi::boot::{MemoryDescriptor, MemoryType, PAGE_SIZE};
	use uefi::mem::memory_map::{MemoryMap, MemoryMapMut};
	use vm_fdt::FdtWriterResult;

	impl super::Fdt<'_> {
		pub fn memory_map(mut self, memory_map: &mut impl MemoryMapMut) -> FdtWriterResult<Self> {
			memory_map.sort();
			info!("Memory map:\n{}", memory_map.display());

			let entries = memory_map
				.entries()
				.filter(|entry| entry.ty == MemoryType::CONVENTIONAL);

			for entry in entries {
				self = self.memory(
					entry.phys_start..entry.phys_start + entry.page_count * PAGE_SIZE as u64,
				)?;
			}

			Ok(self)
		}
	}

	trait MemoryMapExt: MemoryMap {
		fn display(&self) -> MemoryMapDisplay<'_, Self> {
			MemoryMapDisplay { inner: self }
		}
	}

	impl<T> MemoryMapExt for T where T: MemoryMap {}

	struct MemoryMapDisplay<'a, T: ?Sized> {
		inner: &'a T,
	}

	impl<T> fmt::Display for MemoryMapDisplay<'_, T>
	where
		T: MemoryMap,
	{
		fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
			let mut has_fields = false;

			for desc in self.inner.entries() {
				if has_fields {
					f.write_char('\n')?;
				}
				write!(f, "{}", desc.display())?;

				has_fields = true;
			}
			Ok(())
		}
	}

	trait MemoryDescriptorExt {
		fn display(&self) -> MemoryDescriptorDisplay<'_>;
	}

	impl MemoryDescriptorExt for MemoryDescriptor {
		fn display(&self) -> MemoryDescriptorDisplay<'_> {
			MemoryDescriptorDisplay { inner: self }
		}
	}

	struct MemoryDescriptorDisplay<'a> {
		inner: &'a MemoryDescriptor,
	}

	impl fmt::Display for MemoryDescriptorDisplay<'_> {
		fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
			write!(
				f,
				"start: {:#12x}, pages: {:#8x}, type: {:?}",
				self.inner.phys_start, self.inner.page_count, self.inner.ty
			)
		}
	}
}
