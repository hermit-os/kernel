//! Page Tables.
//!
//! This module defines the page tables that we switch to by setting `CR3` to `LEVEL_4_TABLE`.
//!
//! Specifically, we map the first GiB of virtual memory using 512 2-MiB pages.
//! 2-MiB pages are supported on every x86-64 CPU.
//!
//! # Alternatives
//!
//! Filling these structs in assembly at runtime would likely be faster, but also less readable.
//!
//! # Current implementation
//!
//! Some page tables need to point to other page tables, but also contain flags.
//! We do this by adding the flags as bytes to the pointer, which is possible in const-eval.
//! The resulting expression is relocatable.
//!
//! Casting pointers to integers is not possible in const-eval.
//! Asserting that all flag bits in the address are 0 is not possible.
//! Using a bitwise OR (`|`) operation cannot be expressed and would not be relocatable.
//!
//! For details, see this discussion: [rust-lang/rust#51910 (comment)].
//!
//! [rust-lang/rust#51910 (comment)]: https://github.com/rust-lang/rust/issues/51910#issuecomment-1013271838

use core::ops::Range;
use core::{fmt, ptr};

use log::{debug, info, warn};
use x86_64::structures::paging::{
	Mapper, OffsetPageTable, PageSize, PageTableFlags, PhysFrame, Size1GiB, Size2MiB,
};
use x86_64::{PhysAddr, VirtAddr};

use self::cpuid::ExtendedProcessorAndProcessorFeatureIdentifiers;
use crate::arch::x86_64::physicalmem::PhysAlloc;

const TABLE_FLAGS: PageTableFlags = PageTableFlags::PRESENT.union(PageTableFlags::WRITABLE);
const PAGE_FLAGS: PageTableFlags = TABLE_FLAGS.union(PageTableFlags::HUGE_PAGE);

pub static mut LEVEL_4_TABLE: PageTable = {
	let flags = TABLE_FLAGS.bits() as usize;

	let mut page_table = [ptr::null_mut(); _];

	page_table[0] = (&raw mut LEVEL_3_TABLE).wrapping_byte_add(flags).cast();

	PageTable(page_table)
};

static mut LEVEL_3_TABLE: PageTable = {
	let flags = TABLE_FLAGS.bits() as usize;

	let mut page_table = [ptr::null_mut(); _];

	page_table[0] = (&raw mut LEVEL_2_TABLE).wrapping_byte_add(flags).cast();

	PageTable(page_table)
};

static mut LEVEL_2_TABLE: PageTable = {
	let flags: usize = PAGE_FLAGS.bits() as usize;

	let mut page_table = [ptr::null_mut(); _];

	let mut i = 0;
	while i < page_table.len() {
		let addr = i * Size2MiB::SIZE as usize;
		page_table[i] = ptr::with_exposed_provenance_mut(addr + flags);
		i += 1;
	}

	PageTable(page_table)
};

/// Initializes the page tables.
///
/// # Safety
///
/// This function may only be called once before modifying the page tables.
pub unsafe fn init(max_phys_addr: usize) {
	debug!("max_phys_addr = {max_phys_addr:#x}");

	let idents = ExtendedProcessorAndProcessorFeatureIdentifiers::new();
	let has_page_1_gb = idents.has_page_1_gb();

	if has_page_1_gb {
		info!("CPU supports 1-GiB pages.");
	} else {
		warn!("CPU does not support 1-GiB pages.");
	}

	if has_page_1_gb {
		// If supported, we replace the existing mapping of 512 2-MiB pages with 1 1-GiB page.
		//
		// Since the mappings themselves do not change, we don't need to flush the TLB.
		// For details, see Section 5.10.2.3 "Details of TLB Use" in the Intel® 64 and IA-32
		// Architectures Software Developer's Manual Volume 3A: System Programming Guide, Part 1.

		info!("Replacing the 2-MiB pages with a 1-GiB page.");

		let flags: usize = PAGE_FLAGS.bits() as usize;
		let addr = 0;
		unsafe {
			LEVEL_3_TABLE.0[0] = ptr::with_exposed_provenance_mut(addr + flags);
		}
	}

	let addrs = Size1GiB::SIZE as usize..max_phys_addr;

	if has_page_1_gb {
		identity_map::<Size1GiB>(addrs);
	} else {
		identity_map::<Size2MiB>(addrs);
	}
}

fn identity_map<S: PageSize + fmt::Debug>(phys_addrs: Range<usize>)
where
	for<'a> OffsetPageTable<'a>: Mapper<S>,
{
	if phys_addrs.end <= phys_addrs.start {
		return;
	}

	let start_addr = PhysAddr::new(phys_addrs.start as u64);
	let last_addr = PhysAddr::new((phys_addrs.end - 1) as u64);

	let start = PhysFrame::<S>::from_start_address(start_addr).unwrap();
	let last = PhysFrame::<S>::containing_address(last_addr);

	info!("Identity-mapping {start:?}..={last:?}");

	let frames = PhysFrame::range_inclusive(start, last);

	let level_4_table = unsafe { &mut *(&raw mut LEVEL_4_TABLE).cast() };
	let phys_offset = VirtAddr::new(0);
	let mut page_table = unsafe { OffsetPageTable::new(level_4_table, phys_offset) };

	let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

	for frame in frames {
		// SAFETY: We are mapping unused pages to unused frames.
		let result = unsafe { page_table.identity_map(frame, flags, &mut PhysAlloc) };

		// This page was not mapped previously.
		// Thus, we don't need to flush the TLB.
		result.unwrap().ignore();
	}
}

#[repr(align(0x1000))]
#[repr(C)]
pub struct PageTable([*mut (); 512]);

mod cpuid {
	use core::arch::x86_64::CpuidResult;

	/// Extended Processor and Processor Feature Identifiers
	///
	/// We could also use the `raw-cpuid` crate instead, but it is slower, bigger, and less ergonomic.
	#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
	pub struct ExtendedProcessorAndProcessorFeatureIdentifiers(CpuidResult);

	impl ExtendedProcessorAndProcessorFeatureIdentifiers {
		const FUNCTION: u32 = 0x8000_0001;

		pub fn new() -> Self {
			let cpuid_result = core::arch::x86_64::__cpuid(Self::FUNCTION);
			Self(cpuid_result)
		}

		/// 1-GB large page support.
		#[doc(alias = "Page1GB")]
		pub fn has_page_1_gb(&self) -> bool {
			const PAGE_1_GB: u32 = 1 << 26;

			self.0.edx & PAGE_1_GB == PAGE_1_GB
		}
	}
}
