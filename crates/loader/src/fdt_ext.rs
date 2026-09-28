use goblin::elf64::header::{EI_DATA, ELFDATA2LSB, ELFMAG, Header, SELFMAG};

pub trait FdtExt {
	fn find_module_start(&self) -> Option<&'static [u8]>;
	fn find_linux_initrd(&self) -> Option<&'static [u8]>;
	fn find_kernel(&self) -> Option<&'static [u8]>;
}

impl FdtExt for fdt::Fdt<'_> {
	fn find_module_start(&self) -> Option<&'static [u8]> {
		let module = self
			.find_node("/chosen")?
			.children()
			.find(|node| node.name.starts_with("module@"))?;
		let start_ptr = module.reg().unwrap().next().unwrap().starting_address;

		// The reg size of the module nodes is always 0, so we cannot trust them and
		// instead need to parse the ELF header
		let header = unsafe { &*start_ptr.cast::<Header>() };

		if header.e_ident[0..SELFMAG] != ELFMAG[..] {
			return None;
		}

		let len = if header.e_ident[EI_DATA] == ELFDATA2LSB {
			u64::from_le(header.e_shoff)
				+ (u16::from_le(header.e_shentsize) as u64 * u16::from_le(header.e_shnum) as u64)
		} else {
			u64::from_be(header.e_shoff)
				+ (u16::from_be(header.e_shentsize) as u64 * u16::from_be(header.e_shnum) as u64)
		};

		Some(unsafe { core::slice::from_raw_parts(start_ptr, len.try_into().unwrap()) })
	}

	fn find_linux_initrd(&self) -> Option<&'static [u8]> {
		let chosen = self.find_node("/chosen")?;
		let start = chosen.property("linux,initrd-start")?.as_usize()?;
		let end = chosen.property("linux,initrd-end")?.as_usize()?;
		let start_ptr = core::ptr::with_exposed_provenance::<u8>(start);
		let end_ptr = core::ptr::with_exposed_provenance::<u8>(end);
		let len = unsafe { end_ptr.offset_from(start_ptr).try_into().unwrap() };
		Some(unsafe { core::slice::from_raw_parts(start_ptr, len) })
	}

	fn find_kernel(&self) -> Option<&'static [u8]> {
		self.find_module_start()
			.or_else(|| self.find_linux_initrd())
	}
}
