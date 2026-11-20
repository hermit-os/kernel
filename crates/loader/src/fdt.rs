use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::ops::Range;

use vm_fdt::{FdtReserveEntry, FdtWriter, FdtWriterNode, FdtWriterResult};

pub struct Fdt {
	writer: FdtWriter,
	root_node: FdtWriterNode,
	bootargs: Option<String>,
}

impl Fdt {
	pub fn new(platform: &str, mem_reservations: &[FdtReserveEntry]) -> FdtWriterResult<Self> {
		let mut writer = FdtWriter::new_with_mem_reserv(mem_reservations)?;

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
		if let Some(bootargs) = &self.bootargs {
			self.writer.property_string("bootargs", bootargs)?;
		}
		self.writer.end_node(chosen_node)?;

		self.writer.end_node(self.root_node)?;

		self.writer.finish()
	}

	pub fn bootargs(mut self, bootargs: String) -> FdtWriterResult<Self> {
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

	/// Sets the physical address of the EFI SEV-SNP Confidential Computing Blob (CC Blob).
	/// This page contains important information used by an SEV-SNP guest to communicate securely
	/// with the firmware. For the purpose of this loader, however, it is just a pointer we need
	/// to forward to the OS.
	#[cfg(target_os = "uefi")]
	pub fn efi_sev_snp_cc_blob(mut self, efi_sev_snp_cc_blob: u64) -> FdtWriterResult<Self> {
		let cc_blob_node = self.writer.begin_node(&format!(
			"hermit,efi_sev_snp_cc_blob@{efi_sev_snp_cc_blob:x}"
		))?;
		self.writer
			.property_array_u64("reg", &[efi_sev_snp_cc_blob, 1])?;
		self.writer.end_node(cc_blob_node)?;

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

#[cfg(feature = "multiboot")]
mod x86_64 {
	use alloc::format;
	use alloc::vec::Vec;

	use log::info;
	use multiboot::information::{MemoryMapIter, MemoryType};
	use pci_types::{Bar, EndpointHeader, MAX_BARS, PciAddress, PciHeader};
	use vm_fdt::FdtWriterResult;

	use crate::arch::pci::{PCI_MAX_BUS_NUMBER, PCI_MAX_DEVICE_NUMBER, PciConfigRegion};

	impl super::Fdt {
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

		pub fn pci(mut self) -> FdtWriterResult<Self> {
			let fdt = &mut self.writer;

			let pci_node = fdt.begin_node("pci")?;
			fdt.property_string("device_type", "pci")?;

			// TODO: Address cells and size cells should be 3 and 2 respectively. 1 and 1 are only used for compatibility with devicetree output tool.
			fdt.property_u32("#address-cells", 0x1)?;
			fdt.property_u32("#size-cells", 0x1)?;

			info!("Scanning PCI Busses 0 to {}", PCI_MAX_BUS_NUMBER - 1);

			// Hermit only uses PCI for network devices.
			// Therefore, multifunction devices as well as additional bridges are not scanned.
			// We also limit scanning to the first 32 buses.
			let pci_config = PciConfigRegion::new();
			for bus in 0..PCI_MAX_BUS_NUMBER {
				for device in 0..PCI_MAX_DEVICE_NUMBER {
					let pci_address = PciAddress::new(0, bus, device, 0);
					let header = PciHeader::new(pci_address);

					let (vendor_id, device_id) = header.id(&pci_config);
					if device_id != u16::MAX && vendor_id != u16::MAX {
						let addr = ((pci_address.bus() as u32) << 16)
							| ((pci_address.device() as u32) << 11);
						info!("Addr: {:#x}", addr);
						let endpoint = EndpointHeader::from_header(header, &pci_config).unwrap();
						let (_pin, line) = endpoint.interrupt(&pci_config);

						info!("Device ID: {:#x}  Vendor ID: {:#x}", device_id, vendor_id);

						if vendor_id == 0x10ec && (0x8138..=0x8139).contains(&device_id) {
							info!("Network card found.");
							let net_node =
								fdt.begin_node(format!("ethernet@{:x}", addr).as_str())?;

							fdt.property_string("compatible", "realtek,rtl8139")?;
							fdt.property_u32("vendor-id", vendor_id as u32)?;
							fdt.property_u32("device-id", device_id as u32)?;
							fdt.property_u32("interrupts", line as u32)?;

							// The creation of "reg" and "assigned-addresses" properties is based on the
							// PCI Bus Binding to IEEE Std 1275-1994 Revision 2.1 (https://www.devicetree.org/open-firmware/bindings/pci/pci2_1.pdf)
							fdt.property_array_u32(
								"reg",
								&[
									addr,
									0,
									0,
									0,
									0,
									(0x02000010 | addr),
									0,
									0,
									0,
									0x100,
									(0x01000014 | addr),
									0,
									0,
									0,
									0x100,
								],
							)?;

							let mut assigned_addresses: Vec<u32> = Vec::new();
							for i in 0..MAX_BARS {
								if let Some(bar) = endpoint.bar(i.try_into().unwrap(), &pci_config)
								{
									match bar {
										Bar::Io { port } => {
											info!("BAR{:x} IO {{port: {:#X}}}", i, port);
											assigned_addresses.extend(alloc::vec![
												(0x81000014 | addr),
												0,
												port,
												0,
												0x100
											]);
										}
										Bar::Memory32 {
											address,
											size,
											prefetchable,
										} => {
											info!(
												"BAR{:x} Memory32 {{address: {:#X}, size {:#X}, prefetchable: {:?}}}",
												i, address, size, prefetchable
											);
											assigned_addresses.extend(alloc::vec![
												(0x82000010 | addr),
												0,
												address,
												0,
												size
											]);
										}
										Bar::Memory64 {
											address,
											size,
											prefetchable,
										} => {
											info!(
												"BAR{:x} Memory64 {{address: {:#X}, size {:#X}, prefetchable: {:?}}}",
												i, address, size, prefetchable
											);
											assigned_addresses.extend(alloc::vec![
												(0x82000010 | addr),
												(address >> 32) as u32,
												address as u32,
												(size >> 32) as u32,
												size as u32
											]);
										}
									}
								}
							}
							fdt.property_array_u32(
								"assigned-addresses",
								assigned_addresses.as_slice(),
							)?;

							fdt.end_node(net_node)?;
						}
					}
				}
			}

			fdt.end_node(pci_node)?;

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

	impl super::Fdt {
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
