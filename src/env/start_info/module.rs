use core::{ptr, slice};

use free_list::PageRange;

use crate::page_range_ext::PageRangeExt;

/// A module that is passed to the kernel at start.
///
/// This blob is loaded into physical memory by the bootloader or VMM and is described via the start info.
///
/// Examples of modules:
/// - an initramfs
/// - another kernel image
///
/// This type can be thought of as a physical-memory version of `&'static [u8]`.
#[derive(Clone, Copy, Debug)]
pub struct Module {
	phys_addr: usize,
	len: usize,
}

impl Module {
	/// Constructs a new `Module` from a given `phys_addr` and `len`.
	///
	/// # Safety
	///
	/// The physical memory must be identity-mapped and valid for creating a slice.
	#[cfg_attr(not(feature = "hermit-entry"), expect(dead_code))]
	pub unsafe fn new(phys_addr: usize, len: usize) -> Self {
		Self { phys_addr, len }
	}

	/// The physical address of the module.
	#[expect(dead_code)]
	pub fn phys_addr(&self) -> usize {
		self.phys_addr
	}

	/// The length of the module.
	#[expect(dead_code)]
	pub fn len(&self) -> usize {
		self.len
	}

	/// The module as a readable slice.
	#[expect(dead_code)]
	pub fn as_slice(&self) -> &'static [u8] {
		// We require this to be identity-mapped at boot time for now.
		let virt_addr = self.phys_addr;
		let ptr = ptr::with_exposed_provenance(virt_addr);
		// SAFETY: upheld by `Self::new()`
		unsafe { slice::from_raw_parts(ptr, self.len) }
	}

	/// The physical frames covered by this module.
	#[cfg_attr(not(feature = "hermit-entry"), expect(dead_code))]
	pub fn phys_frame_range(&self) -> PageRange {
		let start = self.phys_addr;
		let end = self.phys_addr + self.len;
		PageRange::containing(start, end).unwrap()
	}
}
