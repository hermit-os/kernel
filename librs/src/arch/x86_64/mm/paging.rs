// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//
// MIT License
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
// WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

use core::marker::PhantomData;
use synch::spinlock::*;

extern "C" {
	fn get_pages(npages: usize) -> usize;
	fn ipi_tlb_flush() -> i32;
}

lazy_static! {
	static ref ROOT_PAGETABLE: SpinlockIrqSave<&'static mut PageTable<PML4>> =
		SpinlockIrqSave::new(unsafe { &mut *(0xFFFF_FFFF_FFFF_F000 as *mut PageTable<PML4>) });
}


/// Number of Offset bits of a virtual address, which are shifted away to get its Page Frame Number (PFN).
const PAGE_BITS: usize = 12;

/// Number of bits of the index in each table (PML4, PDPT, PDT, PGT).
const PAGE_MAP_BITS: usize = 9;

/// A mask where PAGE_MAP_BITS are set to calculate a table index.
const PAGE_MAP_MASK: usize = 0x1FF;


bitflags! {
	/// Possible flags for an entry in either table (PML4, PDPT, PDT, PGT)
	///
	/// See Intel Vol. 3A, Tables 4-14 through 4-19
	struct PageTableEntryFlags: usize {
		/// Set if this entry is valid and points to a page or table.
		const PRESENT = 1 << 0;

		/// Set if memory referenced by this entry shall be writable.
		const WRITABLE = 1 << 1;

		/// Set if memory referenced by this entry shall be accessible from user-mode (Ring 3).
		const USER_ACCESSIBLE = 1 << 2;

		/// Set if Write-Through caching shall be enabled for memory referenced by this entry.
		/// Otherwise, Write-Back caching is used.
		const WRITE_THROUGH = 1 << 3;

		/// Set if caching shall be disabled for memory referenced by this entry.
		const CACHE_DISABLE = 1 << 4;

		/// Set if software has accessed this entry (for memory access or address translation).
		const ACCESSED = 1 << 5;

		/// Only for page entries: Set if software has written to the memory referenced by this entry.
		const DIRTY = 1 << 6;

		/// Only for page entries in PDPT or PDT: Set if this entry references a 1GiB (PDPT) or 2MiB (PDT) page.
		const HUGE_PAGE = 1 << 7;

		/// Only for page entries: Set if this address translation is global for all tasks and does not need to
		/// be flushed from the TLB when CR3 is reset.
		const GLOBAL = 1 << 8;

		/// Set if code execution shall be disabled for memory referenced by this entry.
		const EXECUTE_DISABLE = 1 << 63;
    }
}

impl PageTableEntryFlags {
	/// An empty set of flags for unused/zeroed table entries.
	/// Needed as long as empty() is no const function.
	const BLANK: PageTableEntryFlags = PageTableEntryFlags { bits: 0 };
}

/// An entry in either table (PML4, PDPT, PDT, PGT)
struct PageTableEntry {
	/// Physical memory address this entry refers, combined with flags from PageTableEntryFlags.
	physical_address_and_flags: usize
}

impl PageTableEntry {
	/// Zero this entry to mark it as unused.
	fn zero(&mut self) {
		self.physical_address_and_flags = 0;
	}

	/// Returns whether this entry is valid (present).
	fn is_present(&self) -> bool {
		(self.physical_address_and_flags & PageTableEntryFlags::PRESENT.bits()) != 0
	}

	/// Mark this as a valid (present) entry and set address translation and flags.
	///
	/// # Arguments
	///
	/// * `physical_address` - The physical memory address this entry shall translate to
	/// * `flags` - Flags from PageTableEntryFlags (note that the PRESENT and ACCESSED flags are set automatically)
	fn set(&mut self, physical_address: usize, flags: PageTableEntryFlags) {
		self.physical_address_and_flags = physical_address | (PageTableEntryFlags::PRESENT | PageTableEntryFlags::ACCESSED | flags).bits();
	}
}

/// A generic interface to support all possible page sizes.
///
/// This is defined as a subtrait of Copy to enable #[derive(Clone, Copy)] for Page.
/// Currently, deriving implementations for these traits only works if all dependent types implement it as well.
trait PageSize: Copy {
	/// The page size in bytes.
	const SIZE: usize;

	/// The page table level at which a page of this size is mapped (from 0 for PGT through 3 for PML4).
	/// Implemented as a numeric value to enable numeric comparisons.
	const MAP_LEVEL: usize;

	/// Any extra flag that needs to be set to map a page of this size.
	/// For example: PageTableEntryFlags::HUGE_PAGE
	const MAP_EXTRA_FLAG: PageTableEntryFlags;
}

/// A 4KiB page mapped in the PGT.
#[derive(Clone, Copy)]
enum BasePageSize {}
impl PageSize for BasePageSize {
	const SIZE: usize = 4096;
	const MAP_LEVEL: usize = 0;
	const MAP_EXTRA_FLAG: PageTableEntryFlags = PageTableEntryFlags::BLANK;
}

/// A 2MiB page mapped in the PDT.
#[derive(Clone, Copy)]
enum LargePageSize {}
impl PageSize for LargePageSize {
	const SIZE: usize = 2 * 1024 * 1024;
	const MAP_LEVEL: usize = 1;
	const MAP_EXTRA_FLAG: PageTableEntryFlags = PageTableEntryFlags::HUGE_PAGE;
}

/// A 1GiB page mapped in the PDPT.
#[derive(Clone, Copy)]
enum HugePageSize {}
impl PageSize for HugePageSize {
	const SIZE: usize = 1024 * 1024 * 1024;
	const MAP_LEVEL: usize = 2;
	const MAP_EXTRA_FLAG: PageTableEntryFlags = PageTableEntryFlags::HUGE_PAGE;
}

/// A memory page of the size given by S.
#[derive(Clone, Copy)]
struct Page<S: PageSize> {
	/// Virtual memory address of this page.
	/// This is rounded to a page size boundary on creation.
	virtual_address: usize,

	/// Required by Rust to support the S parameter.
	size: PhantomData<S>,
}

impl<S: PageSize> Page<S> {
	/// Flushes this page from the TLB of this CPU.
	unsafe fn flush_from_tlb(&self) {
		asm!("invlpg ($0)" :: "r"(self.virtual_address) : "memory");
	}

	/// Returns whether the given virtual address is a valid one in the x86-64 memory model.
	///
	/// Current x86-64 supports only 48-bit for virtual memory addresses.
	/// This is enforced by requiring bits 63 through 48 to replicate bit 47 (cf. Intel Vol. 1, 3.3.7.1).
	/// As a consequence, the address space is divided into the two valid regions 0x8000_0000_0000
	/// and 0xFFFF_8000_0000_0000.
	fn is_valid_address(virtual_address: usize) -> bool {
		(virtual_address < 0x8000_0000_0000 || virtual_address >= 0xFFFF_8000_0000_0000)
	}

	/// Returns a Page including the given virtual address.
	/// That means, the address is rounded down to a page size boundary.
	fn including_address(virtual_address: usize) -> Self {
		assert!(Self::is_valid_address(virtual_address));
		Self {
			virtual_address: virtual_address & !(S::SIZE - 1),
			size: PhantomData,
		}
	}

	/// Returns a PageIter to iterate from the given first Page to the given last Page (inclusive).
	fn range(first: Self, last: Self) -> PageIter<S> {
		assert!(first.virtual_address <= last.virtual_address);
		PageIter { current: first, last: last }
	}

	/// Returns the index of this page in the table given by L.
	fn table_index<L: PageTableLevel>(&self) -> usize {
		assert!(L::LEVEL >= S::MAP_LEVEL);
		self.virtual_address >> PAGE_BITS >> L::LEVEL * PAGE_MAP_BITS & PAGE_MAP_MASK
	}
}

/// An iterator to walk through a range of pages of size S.
struct PageIter<S: PageSize> {
	current: Page<S>,
	last: Page<S>,
}

impl<S: PageSize> Iterator for PageIter<S> {
	type Item = Page<S>;

	fn next(&mut self) -> Option<Page<S>> {
		if self.current.virtual_address <= self.last.virtual_address {
			let p = self.current;
			self.current.virtual_address += S::SIZE;
			Some(p)
		} else {
			None
		}
	}
}

/// An interface to allow for a generic implementation of struct PageTable for all 4 page tables.
/// Must be implemented by all page tables.
trait PageTableLevel {
	/// Numeric page table level (from 0 for PGT through 3 for PML4) to enable numeric comparisons.
	const LEVEL: usize;
}

/// An interface for page tables with sub page tables (all except PGT).
/// Having both PageTableLevel and PageTableLevelWithSubtables leverages Rust's typing system to provide
/// a next_table_for_page method only for those that have sub page tables.
///
/// Kudos to Philipp Oppermann for the trick!
trait PageTableLevelWithSubtables: PageTableLevel {
	type SubtableLevel;
}

/// The Page Map Level 4 (PML4) table, with numeric level 3 and PDPT subtables.
enum PML4 {}
impl PageTableLevel for PML4 {
	const LEVEL: usize = 3;
}

impl PageTableLevelWithSubtables for PML4 {
	type SubtableLevel = PDPT;
}

/// A Page Directory Pointer Table (PDPT), with numeric level 2 and PDT subtables.
enum PDPT {}
impl PageTableLevel for PDPT {
	const LEVEL: usize = 2;
}

impl PageTableLevelWithSubtables for PDPT {
	type SubtableLevel = PDT;
}

/// A Page Directory Table (PDT), with numeric level 1 and PGT subtables.
enum PDT {}
impl PageTableLevel for PDT {
	const LEVEL: usize = 1;
}

impl PageTableLevelWithSubtables for PDT {
	type SubtableLevel = PGT;
}

/// A Page Table (PGT), with numeric level 0 and no subtables.
enum PGT {}
impl PageTableLevel for PGT {
	const LEVEL: usize = 0;
}

/// Representation of any page table (PML4, PDPT, PDT, PGT) in memory.
/// Parameter L supplies information for Rust's typing system to distinguish between the different tables.
struct PageTable<L> {
	/// Each page table has 512 entries (can be calculated using PAGE_MAP_BITS).
	entries: [PageTableEntry; 1 << PAGE_MAP_BITS],

	/// Required by Rust to support the L parameter.
	level: PhantomData<L>,
}

/// A trait defining methods every page table has to implement.
/// This additional trait is necessary to make use of Rust's specialization feature and provide a default
/// implementation of the map_page method.
trait PageTableMethods {
	fn map_page_to_this_table<S: PageSize>(&mut self, page: Page<S>, physical_address: usize, flags: PageTableEntryFlags) -> bool;
	fn map_page<S: PageSize>(&mut self, page: Page<S>, physical_address: usize, flags: PageTableEntryFlags) -> bool;
}

impl<L: PageTableLevel> PageTableMethods for PageTable<L> {
	/// Maps a single page to the given physical address in this table.
	/// Returns whether an existing entry was updated. You can use this return value to flush TLBs.
	///
	/// Must only be called if a page of this size is mapped at this page table level!
	fn map_page_to_this_table<S: PageSize>(&mut self, page: Page<S>, physical_address: usize, flags: PageTableEntryFlags) -> bool {
		assert!(L::LEVEL == S::MAP_LEVEL);
		let index = page.table_index::<L>();
		let flush = self.entries[index].is_present();

		self.entries[index].set(physical_address, PageTableEntryFlags::DIRTY | S::MAP_EXTRA_FLAG | flags);

		if flush {
			unsafe { page.flush_from_tlb() };
		}

		flush
	}

	/// Maps a single page to the given physical address.
	/// Returns whether an existing entry was updated. You can use this return value to flush TLBs.
	///
	/// This is the default implementation that just calls the map_page_to_this_table method.
	/// It is overridden by a specialized implementation for all tables with sub tables (all except PGT).
	default fn map_page<S: PageSize>(&mut self, page: Page<S>, physical_address: usize, flags: PageTableEntryFlags) -> bool {
		self.map_page_to_this_table::<S>(page, physical_address, flags)
	}
}

impl<L: PageTableLevelWithSubtables> PageTableMethods for PageTable<L> where L::SubtableLevel: PageTableLevel {
	/// Maps a single page to the given physical address.
	/// Returns whether an existing entry was updated. You can use this return value to flush TLBs.
	///
	/// This is the implementation for all tables with subtables (PML4, PDPT, PDT).
	/// It overrides the default implementation above.
	fn map_page<S: PageSize>(&mut self, page: Page<S>, physical_address: usize, flags: PageTableEntryFlags) -> bool {
		assert!(L::LEVEL >= S::MAP_LEVEL);

		if L::LEVEL > S::MAP_LEVEL {
			let next_table = self.next_table_for_page::<S>(page);
			next_table.map_page::<S>(page, physical_address, flags)
		} else {
			// Calling the default implementation from a specialized one is not supported (yet),
			// so we have to resort to an extra function.
			self.map_page_to_this_table::<S>(page, physical_address, flags)
		}
	}
}

impl<L: PageTableLevelWithSubtables> PageTable<L> where L::SubtableLevel: PageTableLevel {
	/// Returns the next subtable for the given page in the page table hierarchy.
	/// If the table does not exist yet, it is created.
	///
	/// Must only be called if a page of this size is mapped in a subtable!
	fn next_table_for_page<S: PageSize>(&mut self, page: Page<S>) -> &mut PageTable<L::SubtableLevel> {
		assert!(L::LEVEL > S::MAP_LEVEL);
		let index = page.table_index::<L>();

		// Calculate the address of the subtable.
		let table_address = self as *const PageTable<L> as usize;
		let next_table_address = (table_address << PAGE_MAP_BITS) | (index << PAGE_BITS);
		let next_table = unsafe { &mut *(next_table_address as *mut PageTable<L::SubtableLevel>) };

		// Does the table exist yet?
		if !self.entries[index].is_present() {
			// Allocate a single 4KiB page for the new entry and mark it as a valid, writable subtable.
			let physical_address = unsafe { get_pages(1) };
			self.entries[index].set(physical_address, PageTableEntryFlags::WRITABLE);

			// Mark all entries as unused in the newly created table.
			for entry in next_table.entries.iter_mut() {
				entry.zero();
			}
		}

		next_table
	}

	/// Maps a range of pages.
	///
	/// # Arguments
	///
	/// * `range` - The range of pages of size S
	/// * `physical_address` - First physical address to map these pages to
	/// * `flags` - Flags from PageTableEntryFlags to set for the page table entry (e.g. WRITABLE or EXECUTE_DISABLE).
	///             The PRESENT, ACCESSED, and DIRTY flags are already set automatically.
	/// * `do_ipi` - Whether to flush the TLB of the other CPUs as well if existing entries were updated.
	fn map_pages<S: PageSize>(&mut self, range: PageIter<S>, physical_address: usize, flags: PageTableEntryFlags, do_ipi: bool) {
		let mut current_physical_address = physical_address;
		let mut send_ipi = false;

		for page in range {
			send_ipi |= self.map_page::<S>(page, current_physical_address, flags);
			current_physical_address += S::SIZE;
		}

		if do_ipi && send_ipi {
			unsafe { ipi_tlb_flush() };
		}
	}
}



#[no_mangle]
pub extern "C" fn __page_map(viraddr: usize, phyaddr: usize, npages: usize, bits: usize, do_ipi: u8) -> i32 {
	let first_page = Page::<BasePageSize>::including_address(viraddr);
	let last_page = Page::<BasePageSize>::including_address(viraddr + (npages - 1) * BasePageSize::SIZE);
	let range = Page::<BasePageSize>::range(first_page, last_page);

	ROOT_PAGETABLE.lock().map_pages(range, phyaddr, PageTableEntryFlags::from_bits_truncate(bits), do_ipi > 0);
	0
}
