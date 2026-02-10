//! Thread-local storage data structures.
//!
//! See _ELF Handling For Thread-Local Storage_ [\[tls\]] for details.
//!
//! - For ARM specifics, see
//!   - _4 ADDENDUM: Thread Local Storage - Addenda to, and Errata in, the ABI for the Arm Architecture_ [\[riscv\]] and
//!   - _Speeding Up Thread-Local Storage Access in Dynamic Libraries in the ARM platform_ [\[paper-lk2006\]].
//! - For RISC-V specifics, see
//!   - _Thread Local Storage - RISC-V ELF Specification - RISC-V ELF psABI Document_ [\[riscv\]].
//! - For x86-64 specifics, see
//!   - _ELF x86-64-ABI psABI_ [\[x86-64 psABI\]] and
//!   - _Thread-Local Storage Descriptors for IA32 and AMD64/EM64T_ [\[RFC-TLSDESC-x86\]]
//!
//! [\[tls\]]: https://akkadia.org/drepper/tls.pdf
//! [\[arm\]]: https://github.com/ARM-software/abi-aa/blob/2025Q1/addenda32/addenda32.rst#addendum-thread-local-storage
//! [\[paper-lk2006\]]: https://www.fsfla.org/~lxoliva/writeups/TLS/paper-lk2006.pdf
//! [\[riscv\]]: https://github.com/riscv-non-isa/riscv-elf-psabi-doc/blob/v1.0/riscv-elf.adoc#thread-local-storage
//! [\[x86-64 psABI\]]: https://gitlab.com/x86-psABIs/x86-64-ABI/-/jobs/artifacts/master/raw/x86-64-ABI/abi.pdf?job=build
//! [\[RFC-TLSDESC-x86\]]: https://www.fsfla.org/~lxoliva/writeups/TLS/RFC-TLSDESC-x86.txt

use core::alloc::Layout;
use core::mem::{self, MaybeUninit};
use core::{ptr, slice};

use hermit_entry::boot_info::TlsInfo;

use self::allocation::Allocation;

/// Thread-local storage data structures.
pub struct Tls {
	_block: Allocation,
	thread_ptr: *mut (),
}

/// Thread control block.
#[repr(C)]
struct Tcb {
	/// Thread pointer.
	#[cfg(target_arch = "x86_64")]
	thread_ptr: *mut (),

	/// Pointer to the dynamic thread vector (dtv).
	///
	/// Currently not needed on Hermit.
	dtv: *mut (),

	/// Implementation-defined TCB data.
	///
	/// Currently not used on Hermit.
	tcb_data: *mut (),
}

impl Tls {
	unsafe fn new(tls_info: TlsInfo) -> Self {
		let start = usize::try_from(tls_info.start).unwrap();
		let filesz = usize::try_from(tls_info.filesz).unwrap();
		let memsz = usize::try_from(tls_info.memsz).unwrap();
		let align = usize::try_from(tls_info.align).unwrap();

		// Get TLS initialization image
		let tls_init_image = {
			let start = ptr::with_exposed_provenance(start);
			unsafe { slice::from_raw_parts(start, filesz) }
		};

		let tcb_layout = Layout::new::<Tcb>().pad_to_align();
		let data_layout = Layout::from_size_align(memsz, align)
			.unwrap()
			.pad_to_align();

		let (layout, tls_offset, tcb_offset) =
			if cfg!(any(target_arch = "aarch64", target_arch = "riscv64")) {
				// AArch64 and 64-bit RISC-V use TLS data structures variant I.

				// Variant I does not guarantee more than 16 bytes of space for the TCB.
				assert_eq!(tcb_layout.size(), 16);

				// Variant I requires the dtv pointer to be at the start of the TCB.
				assert_eq!(mem::offset_of!(Tcb, dtv), 0);

				// In variant I, the TLS data comes after the TCB.
				let (tls_layout, data_offset) = tcb_layout.extend(data_layout).unwrap();
				(tls_layout.pad_to_align(), data_offset, 0)
			} else if cfg!(target_arch = "x86_64") {
				// x86-64 uses TLS data structures variant II.

				// Variant II (on GNU systems) requires the thread pointer to be at the start of the TCB:
				// > For the implementation on GNU systems we can add one more requirement. The
				// > address %gs:0 represents is actually the same as the thread pointer. I.e., the content of
				// > the word addressed via %gs:0 is the address of the very same location.
				#[cfg(target_arch = "x86_64")]
				assert_eq!(mem::offset_of!(Tcb, thread_ptr), 0);

				// In Variant II, the TCB comes after the TLS data.
				let (tls_layout, tcb_offset) = data_layout.extend(tcb_layout).unwrap();
				(tls_layout.pad_to_align(), 0, tcb_offset)
			} else {
				unimplemented!()
			};

		let mut block = Allocation::new(layout).unwrap();

		// Initialize the beginning of the TLS block with the TLS initialization image.
		block.as_mut_slice()[tls_offset..][..tls_init_image.len()].copy_from_slice(tls_init_image);

		// Fill the rest of the TLS block with zeros.
		block.as_mut_slice()[tls_offset..][tls_init_image.len()..data_layout.size()]
			.fill(MaybeUninit::new(0));

		let thread_ptr = if cfg!(target_arch = "riscv64") {
			// On RISC-V, `tp` points to the address one past the end of the TCB.
			unsafe { block.as_mut_ptr().add(tls_offset).cast() }
		} else if cfg!(target_arch = "aarch64") {
			// For variant I, `tp` points to the start of the block.
			block.as_mut_ptr().cast()
		} else if cfg!(target_arch = "x86_64") {
			// For variant II, `tp` points to the TCB after the TLS data.
			unsafe { block.as_mut_ptr().add(tcb_offset).cast() }
		} else {
			unimplemented!()
		};

		let tcb_ptr = unsafe { block.as_mut_ptr().add(tcb_offset).cast::<Tcb>() };
		let tcb = Tcb {
			#[cfg(target_arch = "x86_64")]
			thread_ptr,
			dtv: ptr::null_mut(),
			tcb_data: ptr::null_mut(),
		};
		unsafe {
			tcb_ptr.write(tcb);
		}

		Self {
			_block: block,
			thread_ptr,
		}
	}

	pub fn from_env() -> Option<Self> {
		let tls_info = crate::env::boot_info().load_info.tls_info?;
		let this = unsafe { Self::new(tls_info) };
		Some(this)
	}

	pub fn thread_ptr(&self) -> *mut () {
		self.thread_ptr
	}

	/// Sets the thread pointer register to this TLS's thread pointer value.
	///
	/// This should only be used for the idle task. Since the idle task is
	/// already running, we don't create a new stack frame that we would put
	/// the thread pointer value into otherwise.
	///
	/// The idle task does not enter userspace. That's why it does not need a
	/// TLS most of the time. In special situations such as instrumenting the
	/// kernel, the tracer or profiler or `mcount` implementation such as
	/// rftrace might use TLS for differentiating between the idle task and
	/// other tasks.
	pub fn set_thread_ptr(&self) {
		cfg_if::cfg_if! {
			if #[cfg(target_arch = "aarch64")] {
				use aarch64_cpu::registers::{TPIDR_EL0, Writeable};

				let addr = self.thread_ptr().expose_provenance();
				TPIDR_EL0.set(addr.try_into().unwrap());
			} else if #[cfg(target_arch = "riscv64")] {
				unsafe {
					core::arch::asm!(
						"mv tp, {}",
						in(reg) self.thread_ptr().expose_provenance(),
						options(nomem, nostack, preserves_flags),
					);
				}
			} else if #[cfg(target_arch = "x86_64")] {
				use crate::arch::x86_64::kernel::processor;

				let addr = self.thread_ptr().expose_provenance();
				processor::writefs(addr);
			}
		}
	}
}

mod allocation {
	use core::alloc::Layout;
	use core::mem::MaybeUninit;
	use core::slice;

	pub struct Allocation {
		ptr: *mut u8,
		layout: Layout,
	}

	impl Allocation {
		pub fn new(layout: Layout) -> Option<Self> {
			let ptr = unsafe { ::alloc::alloc::alloc(layout) };

			if ptr.is_null() {
				return None;
			}

			Some(Self { ptr, layout })
		}

		pub fn as_mut_ptr(&mut self) -> *mut u8 {
			self.ptr
		}

		pub fn as_mut_slice(&mut self) -> &mut [MaybeUninit<u8>] {
			unsafe { slice::from_raw_parts_mut(self.ptr.cast(), self.layout.size()) }
		}
	}

	impl Drop for Allocation {
		fn drop(&mut self) {
			unsafe {
				::alloc::alloc::dealloc(self.ptr, self.layout);
			}
		}
	}
}
