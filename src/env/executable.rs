//! Information about the executable.
//!
//! This module provides information about the currently running loaded
//! executable image through different reserved linker symbols.

#[cfg(not(feature = "common-os"))]
pub mod tls {
	use core::alloc::Layout;
	use core::{ptr, slice};

	use elf::abi;
	use elf::file::Elf64_Ehdr;
	use elf::segment::Elf64_Phdr;

	#[derive(Debug)]
	pub struct TlsInfo<'a> {
		pub image: &'a [u8],
		pub layout: Layout,
	}

	impl TlsInfo<'_> {
		pub fn from_env() -> Option<Self> {
			let ehdr = elf_symbols::elf_header().cast::<Elf64_Ehdr>();
			let ehdr = unsafe { ehdr.as_ref().unwrap() };
			ehdr.sanity_check_ident();

			let phdrs = unsafe { ehdr.phdrs() };
			let tls_phdr = phdrs.iter().find(|phdr| phdr.p_type == abi::PT_TLS)?;
			let executable_start = elf_symbols::executable_start().expose_provenance() as u64;

			let start = usize::try_from(executable_start + tls_phdr.p_vaddr).unwrap();
			let filesz = usize::try_from(tls_phdr.p_filesz).unwrap();
			let memsz = usize::try_from(tls_phdr.p_memsz).unwrap();
			let align = usize::try_from(tls_phdr.p_align).unwrap();

			let start = ptr::with_exposed_provenance(start);
			let image = unsafe { slice::from_raw_parts(start, filesz) };

			let layout = Layout::from_size_align(memsz, align)
				.unwrap()
				.pad_to_align();

			Some(Self { image, layout })
		}
	}

	#[expect(non_camel_case_types)]
	trait Elf64_EhdrExt {
		fn sanity_check_ident(&self);

		unsafe fn phdrs(&self) -> &[Elf64_Phdr];
	}

	impl Elf64_EhdrExt for Elf64_Ehdr {
		fn sanity_check_ident(&self) {
			let ident = &self.e_ident;

			let magic = &ident[..abi::EI_CLASS];
			assert_eq!(magic, abi::ELFMAGIC);

			let version = ident[abi::EI_VERSION];
			assert_eq!(version, abi::EV_CURRENT);

			let class = ident[abi::EI_CLASS];
			assert_eq!(class, abi::ELFCLASS64);

			/// 2's complement values, with native endianness.
			pub const ELFDATA2NATIVE: u8 = if cfg!(target_endian = "little") {
				abi::ELFDATA2LSB
			} else if cfg!(target_endian = "big") {
				abi::ELFDATA2MSB
			} else {
				unreachable!()
			};

			let data = ident[abi::EI_DATA];
			assert_eq!(data, ELFDATA2NATIVE);

			/// Stand-alone (embedded) ABI
			pub const ELFOSABI_STANDALONE: u8 = 255;

			let osabi = ident[abi::EI_OSABI];
			// For some reason `x86_64-unknown-none` uses `ELFOSABI_GNU`.
			// We need to allow this for `no_std` applications such as our integration tests.
			assert!(osabi == ELFOSABI_STANDALONE || osabi == abi::ELFOSABI_GNU);

			let abiversion = ident[abi::EI_ABIVERSION];
			assert_eq!(abiversion, 0);
		}

		unsafe fn phdrs(&self) -> &[Elf64_Phdr] {
			let ptr = unsafe {
				ptr::from_ref(self)
					.byte_add(self.e_phoff as usize)
					.cast::<Elf64_Phdr>()
			};
			let len = self.e_phnum as usize;
			unsafe { slice::from_raw_parts(ptr, len) }
		}
	}
}
