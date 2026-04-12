use core::ops::Range;

pub fn executable_ptr_range() -> Range<*mut ()> {
	executable_start()..executable_end()
}

pub fn log_segments() {
	info!("Executable start:  {:p}", executable_start());
	info!("Text segment end:  {:p}", text_end());
	info!("Data segment end:  {:p}", data_end());
	info!("BSS segment start: {:p}", bss_start());
	info!("Executable end:    {:p}", executable_end());
}

fn executable_start() -> *mut () {
	unsafe extern "C" {
		/// Start of the executable.
		///
		/// The address of `__executable_start` is the location of the first
		/// loadable segment. Apart from changelogs, it is not documented, but
		/// defined by:
		///
		/// - ld: [binutils-gdb@`c6d3b05`]
		/// - gold: [binutils-gdb@`f6ce93d`]
		/// - lld: [llvm/llvm-project@`0e454a9`]
		/// - mold: [rui314/mold@`5f492fe`]
		/// - Wild: [wild-linker/wild@`0a21948`]
		///
		/// [binutils-gdb@`c6d3b05`]: https://sourceware.org/git/?p=binutils-gdb.git;a=commit;h=c6d3b05fe766fe33bb96b8850559c9ada7296dd4
		/// [binutils-gdb@`f6ce93d`]: https://sourceware.org/git/?p=binutils-gdb.git;a=commit;h=f6ce93d6e999d1a0c450c5e71c5b3468e6217f0a
		/// [llvm/llvm-project@`0e454a9`]: https://github.com/llvm/llvm-project/commit/0e454a9837c312807e8791dfcd8607cbc18d4359
		/// [rui314/mold@`5f492fe`]: https://github.com/rui314/mold/commit/5f492fea708029656ddaea8e9b53a8fc3b503b7a
		/// [wild-linker/wild@`0a21948`]: https://github.com/wild-linker/wild/commit/0a219486590a3349c803377170beed9afe759210
		static mut __executable_start: u8;
	}

	(&raw mut __executable_start).cast::<()>()
}

fn executable_end() -> *mut () {
	unsafe extern "C" {
		/// End of the executable.
		///
		/// The address of `_end` is the first location after the last loadable
		/// segment. For details, see [etext(3C)]. It is defined by:
		///
		/// - ld: [binutils-gdb@`252b513`]
		/// - gold: [binutils-gdb@`ead1e42`]
		/// - lld: [llvm/llvm-project@`b044af5`]
		/// - mold: [rui314/mold@`694ae9a`]
		/// - Wild: [wild-linker/wild@`fb7da78`]
		///
		/// [etext(3C)]: https://docs.oracle.com/cd/E86824_01/html/E54766/etext-3c.html
		/// [binutils-gdb@`252b513`]: https://sourceware.org/git/?p=binutils-gdb.git;a=commit;h=252b5132c753830d5fd56823373aed85f2a0db63
		/// [binutils-gdb@`ead1e42`]: https://sourceware.org/git/?p=binutils-gdb.git;a=commit;h=ead1e4244a55707685d105c662a9a1faf5d122fe
		/// [llvm/llvm-project@`b044af5`]: https://github.com/llvm/llvm-project/commit/b044af50f28209ff4eeed8fa4614e78969d8df74
		/// [rui314/mold@`694ae9a`]: https://github.com/rui314/mold/commit/694ae9a9c5282809db85dd9f3858e8f697989843
		/// [wild-linker/wild@`fb7da78`]: https://github.com/wild-linker/wild/commit/fb7da7841ad9e64e2cd1128a62d23d488dae921d
		static mut _end: u8;
	}

	(&raw mut _end).cast::<()>()
}

fn text_end() -> *mut () {
	unsafe extern "C" {
		/// End of the text segment.
		///
		/// The address of `_etext` is the first location after the last read-only
		/// loadable segment. For details, see [etext(3C)].
		///
		/// [etext(3C)]: https://docs.oracle.com/cd/E86824_01/html/E54766/etext-3c.html
		static mut _etext: u8;
	}

	(&raw mut _etext).cast::<()>()
}

fn data_end() -> *mut () {
	unsafe extern "C" {
		/// End of the data segment.
		///
		/// The address of `_edata` is the first location after the last read-write
		/// loadable segment. For details, see [etext(3C)].
		///
		/// [etext(3C)]: https://docs.oracle.com/cd/E86824_01/html/E54766/etext-3c.html
		static mut _edata: u8;
	}

	(&raw mut _edata).cast::<()>()
}

fn bss_start() -> *mut () {
	unsafe extern "C" {
		static mut __bss_start: u8;
	}

	(&raw mut __bss_start).cast::<()>()
}

#[cfg(not(feature = "common-os"))]
pub mod tls {
	use core::alloc::Layout;
	use core::{ptr, slice};

	use elf::abi;
	use elf::file::Elf64_Ehdr;
	use elf::segment::Elf64_Phdr;

	use crate::env;

	#[derive(Debug)]
	pub struct TlsInfo<'a> {
		pub image: &'a [u8],
		pub layout: Layout,
	}

	impl TlsInfo<'_> {
		pub fn from_env() -> Option<Self> {
			let ehdr = ehdr();
			ehdr.sanity_check_ident();

			let phdrs = unsafe { ehdr.phdrs() };
			let tls_phdr = phdrs.iter().find(|phdr| phdr.p_type == abi::PT_TLS)?;
			let executable_start = env::executable_ptr_range().start.expose_provenance() as u64;

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

	fn ehdr() -> &'static Elf64_Ehdr {
		unsafe extern "C" {
			/// ELF file header.
			///
			/// Apart from changelogs, it is not documented, but defined by:
			///
			/// - ld: [binutils-gdb@`62655c7`]
			/// - gold: [binutils-gdb@`eabc84f`]
			/// - lld: [llvm/llvm-project@`4f7a5c3`]
			/// - mold: [rui314/mold@`ccc7f83`]
			/// - Wild: [wild-linker/wild@`fb7da78`]
			///
			/// [binutils-gdb@`62655c7`]: https://sourceware.org/git/?p=binutils-gdb.git;a=commit;h=62655c7b8bfc33e6c12694f439ff8f7e8da3005a
			/// [binutils-gdb@`eabc84f`]: https://sourceware.org/git/?p=binutils-gdb.git;a=commit;h=eabc84f4848311a68f65df04e428c8b53a92f1c0
			/// [llvm/llvm-project@`4f7a5c3`]: https://github.com/llvm/llvm-project/commit/4f7a5c3bb429c945ff4e456478b5532f828d1143
			/// [rui314/mold@`ccc7f83`]: https://github.com/rui314/mold/commit/ccc7f83cde954048424fa488464be41f210e60d1
			/// [wild-linker/wild@`fb7da78`]: https://github.com/wild-linker/wild/commit/fb7da7841ad9e64e2cd1128a62d23d488dae921d
			static __ehdr_start: Elf64_Ehdr;
		}

		unsafe { &__ehdr_start }
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
