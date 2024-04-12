use alloc::format;
use alloc::vec::Vec;
use core::fmt::{self, Write};

use log::info;
use uefi::boot::{MemoryDescriptor, MemoryType, PAGE_SIZE};
use uefi::mem::memory_map::{MemoryMap, MemoryMapMut};
use vm_fdt::{FdtWriter, FdtWriterNode, FdtWriterResult};

pub struct Fdt {
	writer: FdtWriter,
	root_node: FdtWriterNode,
}

impl Fdt {
	pub fn new() -> FdtWriterResult<Self> {
		let mut writer = FdtWriter::new()?;

		let root_node = writer.begin_node("")?;
		writer.property_string("compatible", "hermit,uefi")?;
		writer.property_u32("#address-cells", 0x2)?;
		writer.property_u32("#size-cells", 0x2)?;

		let chosen_node = writer.begin_node("chosen")?;
		writer.end_node(chosen_node)?;

		Ok(Self { writer, root_node })
	}

	pub fn finish(mut self) -> FdtWriterResult<Vec<u8>> {
		self.writer.end_node(self.root_node)?;

		self.writer.finish()
	}

	pub fn rsdp(mut self, rsdp: u64) -> FdtWriterResult<Self> {
		let rsdp_node = self.writer.begin_node(&format!("hermit,rsdp@{rsdp:x}"))?;
		self.writer.property_array_u64("reg", &[rsdp, 1])?;
		self.writer.end_node(rsdp_node)?;

		Ok(self)
	}

	pub fn memory_map(mut self, memory_map: &mut impl MemoryMapMut) -> FdtWriterResult<Self> {
		memory_map.sort();
		info!("Memory map:\n{}", memory_map.display());

		let entries = memory_map
			.entries()
			.filter(|entry| entry.ty == MemoryType::CONVENTIONAL);

		for entry in entries {
			let memory_node = self
				.writer
				.begin_node(format!("memory@{:x}", entry.phys_start).as_str())?;
			self.writer.property_string("device_type", "memory")?;
			self.writer.property_array_u64(
				"reg",
				&[entry.phys_start, entry.page_count * PAGE_SIZE as u64],
			)?;
			self.writer.end_node(memory_node)?;
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

impl<'a, T> fmt::Display for MemoryMapDisplay<'a, T>
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

impl<'a> fmt::Display for MemoryDescriptorDisplay<'a> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"start: {:#12x}, pages: {:#8x}, type: {:?}",
			self.inner.phys_start, self.inner.page_count, self.inner.ty
		)
	}
}
