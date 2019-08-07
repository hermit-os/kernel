// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

#![allow(dead_code)]

use arch::x86_64::kernel::apic;
use arch::x86_64::kernel::get_mbinfo;
use arch::x86_64::kernel::irq;
use arch::x86_64::kernel::is_uhyve;
use arch::x86_64::kernel::percore::*;
use arch::x86_64::kernel::processor;
use arch::x86_64::mm::physicalmem;
use arch::x86_64::mm::virtualmem;
use core::marker::PhantomData;
use core::mem;
use core::ptr;
use environment;
use hermit_multiboot::Multiboot;
use mm;
use scheduler;
use x86::controlregs;
use x86::irq::PageFaultError;

extern "C" {
	#[linkage = "extern_weak"]
	static runtime_osinit: *const u8;
}

/// Uhyve's address of the initial GDT
const BOOT_GDT: usize = 0x1000;

/// Pointer to the root page table (PML4)
const PML4_ADDRESS: *mut PageTable<PML4> = 0xFFFF_FFFF_FFFF_F000 as *mut PageTable<PML4>;

/// Number of Offset bits of a virtual address for a 4 KiB page, which are shifted away to get its Page Frame Number (PFN).
const PAGE_BITS: usize = 12;

/// Number of bits of the index in each table (PML4, PDPT, PD, PT).
const PAGE_MAP_BITS: usize = 9;

/// A mask where PAGE_MAP_BITS are set to calculate a table index.
const PAGE_MAP_MASK: usize = 0x1FF;

bitflags! {
	/// Possible flags for an entry in either table (PML4, PDPT, PD, PT)
	///
	/// See Intel Vol. 3A, Tables 4-14 through 4-19
	pub struct PageTableEntryFlags: usize {
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

		/// Only for page entries in PDPT or PDT: Set if this entry references a 1 GiB (PDPT) or 2 MiB (PDT) page.
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

	pub fn device(&mut self) -> &mut Self {
		self.insert(PageTableEntryFlags::CACHE_DISABLE);
		self
	}

	pub fn normal(&mut self) -> &mut Self {
		self.remove(PageTableEntryFlags::CACHE_DISABLE);
		self
	}

	pub fn read_only(&mut self) -> &mut Self {
		self.remove(PageTableEntryFlags::WRITABLE);
		self
	}

	pub fn writable(&mut self) -> &mut Self {
		self.insert(PageTableEntryFlags::WRITABLE);
		self
	}

	pub fn execute_disable(&mut self) -> &mut Self {
		self.insert(PageTableEntryFlags::EXECUTE_DISABLE);
		self
	}
}

/// An entry in either table (PML4, PDPT, PD, PT)
#[derive(Clone, Copy)]
pub struct PageTableEntry {
	/// Physical memory address this entry refers, combined with flags from PageTableEntryFlags.
	physical_address_and_flags: usize,
}

impl PageTableEntry {
	/// Return the stored physical address.
	pub fn address(self) -> usize {
		self.physical_address_and_flags
			& !(BasePageSize::SIZE - 1)
			& !(PageTableEntryFlags::EXECUTE_DISABLE).bits()
	}

	/// Returns whether this entry is valid (present).
	fn is_present(self) -> bool {
		(self.physical_address_and_flags & PageTableEntryFlags::PRESENT.bits()) != 0
	}

	/// Returns `true` if the page is a huge page
	fn is_huge(self) -> bool {
		(self.physical_address_and_flags & PageTableEntryFlags::HUGE_PAGE.bits()) != 0
	}

	/// Returns `true` if the page is accessible from the user space
	fn is_user(self) -> bool {
		(self.physical_address_and_flags & PageTableEntryFlags::USER_ACCESSIBLE.bits()) != 0
	}

	/// Mark this as a valid (present) entry and set address translation and flags.
	///
	/// # Arguments
	///
	/// * `physical_address` - The physical memory address this entry shall translate to
	/// * `flags` - Flags from PageTableEntryFlags (note that the PRESENT and ACCESSED flags are set automatically)
	fn set(&mut self, physical_address: usize, flags: PageTableEntryFlags) {
		if flags.contains(PageTableEntryFlags::HUGE_PAGE) {
			// HUGE_PAGE may indicate a 2 MiB or 1 GiB page.
			// We don't know this here, so we can only verify that at least the offset bits for a 2 MiB page are zero.
			assert!(
				physical_address % LargePageSize::SIZE == 0,
				"Physical address is not on a 2 MiB page boundary (physical_address = {:#X})",
				physical_address
			);
		} else {
			// Verify that the offset bits for a 4 KiB page are zero.
			assert!(
				physical_address % BasePageSize::SIZE == 0,
				"Physical address is not on a 4 KiB page boundary (physical_address = {:#X})",
				physical_address
			);
		}

		// Verify that the physical address does not exceed the CPU's physical address width.
		assert!(
			physical_address >> processor::get_physical_address_bits() == 0,
			"Physical address exceeds CPU's physical address width (physical_address = {:#X})",
			physical_address
		);

		let mut flags_to_set = flags;
		flags_to_set.insert(PageTableEntryFlags::PRESENT);
		flags_to_set.insert(PageTableEntryFlags::ACCESSED);
		self.physical_address_and_flags = physical_address | flags_to_set.bits();
	}
}

/// A generic interface to support all possible page sizes.
///
/// This is defined as a subtrait of Copy to enable #[derive(Clone, Copy)] for Page.
/// Currently, deriving implementations for these traits only works if all dependent types implement it as well.
pub trait PageSize: Copy {
	/// The page size in bytes.
	const SIZE: usize;

	/// The page table level at which a page of this size is mapped (from 0 for PT through 3 for PML4).
	/// Implemented as a numeric value to enable numeric comparisons.
	const MAP_LEVEL: usize;

	/// Any extra flag that needs to be set to map a page of this size.
	/// For example: PageTableEntryFlags::HUGE_PAGE
	const MAP_EXTRA_FLAG: PageTableEntryFlags;
}

/// A 4 KiB page mapped in the PT.
#[derive(Clone, Copy)]
pub enum BasePageSize {}
impl PageSize for BasePageSize {
	const SIZE: usize = 4096;
	const MAP_LEVEL: usize = 0;
	const MAP_EXTRA_FLAG: PageTableEntryFlags = PageTableEntryFlags::BLANK;
}

/// A 2 MiB page mapped in the PD.
#[derive(Clone, Copy)]
pub enum LargePageSize {}
impl PageSize for LargePageSize {
	const SIZE: usize = 2 * 1024 * 1024;
	const MAP_LEVEL: usize = 1;
	const MAP_EXTRA_FLAG: PageTableEntryFlags = PageTableEntryFlags::HUGE_PAGE;
}

/// A 1 GiB page mapped in the PDPT.
#[derive(Clone, Copy)]
pub enum HugePageSize {}
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
	/// Return the stored virtual address.
	fn address(self) -> usize {
		self.virtual_address
	}

	/// Flushes this page from the TLB of this CPU.
	fn flush_from_tlb(self) {
		unsafe {
			asm!("invlpg ($0)" :: "r"(self.virtual_address) : "memory" : "volatile");
		}
	}

	/// Returns whether the given virtual address is a valid one in the x86-64 memory model.
	///
	/// Current x86-64 supports only 48-bit for virtual memory addresses.
	/// This is enforced by requiring bits 63 through 48 to replicate bit 47 (cf. Intel Vol. 1, 3.3.7.1).
	/// As a consequence, the address space is divided into the two valid regions 0x8000_0000_0000
	/// and 0xFFFF_8000_0000_0000.
	///
	/// Although we could make this check depend on the actual linear address width from the CPU,
	/// any extension above 48-bit would require a new page table level, which we don't implement.
	fn is_valid_address(virtual_address: usize) -> bool {
		(virtual_address < 0x8000_0000_0000 || virtual_address >= 0xFFFF_8000_0000_0000)
	}

	/// Returns a Page including the given virtual address.
	/// That means, the address is rounded down to a page size boundary.
	fn including_address(virtual_address: usize) -> Self {
		assert!(
			Self::is_valid_address(virtual_address),
			"Virtual address {:#X} is invalid",
			virtual_address
		);

		if S::SIZE == 1024 * 1024 * 1024 {
			assert!(processor::supports_1gib_pages());
		}

		Self {
			virtual_address: align_down!(virtual_address, S::SIZE),
			size: PhantomData,
		}
	}

	/// Returns a PageIter to iterate from the given first Page to the given last Page (inclusive).
	fn range(first: Self, last: Self) -> PageIter<S> {
		assert!(first.virtual_address <= last.virtual_address);
		PageIter {
			current: first,
			last: last,
		}
	}

	/// Returns the index of this page in the table given by L.
	fn table_index<L: PageTableLevel>(self) -> usize {
		assert!(L::LEVEL >= S::MAP_LEVEL);
		self.virtual_address >> PAGE_BITS >> (L::LEVEL * PAGE_MAP_BITS) & PAGE_MAP_MASK
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
	/// Numeric page table level (from 0 for PT through 3 for PML4) to enable numeric comparisons.
	const LEVEL: usize;
}

/// An interface for page tables with sub page tables (all except PT).
/// Having both PageTableLevel and PageTableLevelWithSubtables leverages Rust's typing system to provide
/// a subtable method only for those that have sub page tables.
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
	type SubtableLevel = PD;
}

/// A Page Directory (PD), with numeric level 1 and PT subtables.
enum PD {}
impl PageTableLevel for PD {
	const LEVEL: usize = 1;
}

impl PageTableLevelWithSubtables for PD {
	type SubtableLevel = PT;
}

/// A Page Table (PT), with numeric level 0 and no subtables.
enum PT {}
impl PageTableLevel for PT {
	const LEVEL: usize = 0;
}

/// Representation of any page table (PML4, PDPT, PD, PT) in memory.
/// Parameter L supplies information for Rust's typing system to distinguish between the different tables.
#[repr(C)]
struct PageTable<L> {
	/// Each page table has 512 entries (can be calculated using PAGE_MAP_BITS).
	entries: [PageTableEntry; 1 << PAGE_MAP_BITS],

	/// Required by Rust to support the L parameter.
	level: PhantomData<L>,
}

/// A trait defining methods every page table has to implement.
/// This additional trait is necessary to make use of Rust's specialization feature and provide a default
/// implementation of some methods.
trait PageTableMethods {
	fn get_page_table_entry<S: PageSize>(&self, page: Page<S>) -> Option<PageTableEntry>;
	fn map_page_in_this_table<S: PageSize>(
		&mut self,
		page: Page<S>,
		physical_address: usize,
		flags: PageTableEntryFlags,
	) -> bool;
	fn map_page<S: PageSize>(
		&mut self,
		page: Page<S>,
		physical_address: usize,
		flags: PageTableEntryFlags,
	) -> bool;
}

impl<L: PageTableLevel> PageTableMethods for PageTable<L> {
	/// Maps a single page in this table to the given physical address.
	/// Returns whether an existing entry was updated. You can use this return value to flush TLBs.
	///
	/// Must only be called if a page of this size is mapped at this page table level!
	fn map_page_in_this_table<S: PageSize>(
		&mut self,
		page: Page<S>,
		physical_address: usize,
		flags: PageTableEntryFlags,
	) -> bool {
		assert!(L::LEVEL == S::MAP_LEVEL);
		let index = page.table_index::<L>();
		let flush = self.entries[index].is_present();

		self.entries[index].set(
			physical_address,
			PageTableEntryFlags::DIRTY | S::MAP_EXTRA_FLAG | flags,
		);

		if flush {
			page.flush_from_tlb();
		}

		flush
	}

	/// Returns the PageTableEntry for the given page if it is present, otherwise returns None.
	///
	/// This is the default implementation called only for PT.
	/// It is overridden by a specialized implementation for all tables with sub tables (all except PT).
	default fn get_page_table_entry<S: PageSize>(&self, page: Page<S>) -> Option<PageTableEntry> {
		assert!(L::LEVEL == S::MAP_LEVEL);
		let index = page.table_index::<L>();

		if self.entries[index].is_present() {
			Some(self.entries[index])
		} else {
			None
		}
	}

	/// Maps a single page to the given physical address.
	/// Returns whether an existing entry was updated. You can use this return value to flush TLBs.
	///
	/// This is the default implementation that just calls the map_page_in_this_table method.
	/// It is overridden by a specialized implementation for all tables with sub tables (all except PT).
	default fn map_page<S: PageSize>(
		&mut self,
		page: Page<S>,
		physical_address: usize,
		flags: PageTableEntryFlags,
	) -> bool {
		self.map_page_in_this_table::<S>(page, physical_address, flags)
	}
}

impl<L: PageTableLevelWithSubtables> PageTableMethods for PageTable<L>
where
	L::SubtableLevel: PageTableLevel,
{
	/// Returns the PageTableEntry for the given page if it is present, otherwise returns None.
	///
	/// This is the implementation for all tables with subtables (PML4, PDPT, PDT).
	/// It overrides the default implementation above.
	fn get_page_table_entry<S: PageSize>(&self, page: Page<S>) -> Option<PageTableEntry> {
		assert!(L::LEVEL >= S::MAP_LEVEL);
		let index = page.table_index::<L>();

		if self.entries[index].is_present() {
			if L::LEVEL > S::MAP_LEVEL {
				let subtable = self.subtable::<S>(page);
				subtable.get_page_table_entry::<S>(page)
			} else {
				Some(self.entries[index])
			}
		} else {
			None
		}
	}

	/// Maps a single page to the given physical address.
	/// Returns whether an existing entry was updated. You can use this return value to flush TLBs.
	///
	/// This is the implementation for all tables with subtables (PML4, PDPT, PDT).
	/// It overrides the default implementation above.
	fn map_page<S: PageSize>(
		&mut self,
		page: Page<S>,
		physical_address: usize,
		flags: PageTableEntryFlags,
	) -> bool {
		assert!(L::LEVEL >= S::MAP_LEVEL);

		if L::LEVEL > S::MAP_LEVEL {
			let index = page.table_index::<L>();

			// Does the table exist yet?
			if !self.entries[index].is_present() {
				// Allocate a single 4 KiB page for the new entry and mark it as a valid, writable subtable.
				let physical_address = physicalmem::allocate(BasePageSize::SIZE);
				self.entries[index].set(physical_address, PageTableEntryFlags::WRITABLE);

				// Mark all entries as unused in the newly created table.
				let subtable = self.subtable::<S>(page);
				for entry in subtable.entries.iter_mut() {
					entry.physical_address_and_flags = 0;
				}
			}

			let subtable = self.subtable::<S>(page);
			subtable.map_page::<S>(page, physical_address, flags)
		} else {
			// Calling the default implementation from a specialized one is not supported (yet),
			// so we have to resort to an extra function.
			self.map_page_in_this_table::<S>(page, physical_address, flags)
		}
	}
}

impl<L: PageTableLevelWithSubtables> PageTable<L>
where
	L::SubtableLevel: PageTableLevel,
{
	/// Returns the next subtable for the given page in the page table hierarchy.
	///
	/// Must only be called if a page of this size is mapped in a subtable!
	fn subtable<S: PageSize>(&self, page: Page<S>) -> &mut PageTable<L::SubtableLevel> {
		assert!(L::LEVEL > S::MAP_LEVEL);

		// Calculate the address of the subtable.
		let index = page.table_index::<L>();
		let table_address = self as *const PageTable<L> as usize;
		let subtable_address = (table_address << PAGE_MAP_BITS) | (index << PAGE_BITS);
		unsafe { &mut *(subtable_address as *mut PageTable<L::SubtableLevel>) }
	}

	/// Maps a continuous range of pages.
	///
	/// # Arguments
	///
	/// * `range` - The range of pages of size S
	/// * `physical_address` - First physical address to map these pages to
	/// * `flags` - Flags from PageTableEntryFlags to set for the page table entry (e.g. WRITABLE or EXECUTE_DISABLE).
	///             The PRESENT, ACCESSED, and DIRTY flags are already set automatically.
	fn map_pages<S: PageSize>(
		&mut self,
		range: PageIter<S>,
		physical_address: usize,
		flags: PageTableEntryFlags,
	) {
		let mut current_physical_address = physical_address;
		let mut send_ipi = false;

		for page in range {
			send_ipi |= self.map_page::<S>(page, current_physical_address, flags);
			current_physical_address += S::SIZE;
		}

		if send_ipi {
			apic::ipi_tlb_flush();
		}
	}
}

fn map_page_on_demand<S: PageSize>(virtual_address: usize) {
	let physical_address = physicalmem::allocate_aligned(S::SIZE, S::SIZE);
	let root_pagetable = unsafe { &mut *PML4_ADDRESS };
	let page = Page::<S>::including_address(virtual_address);

	trace!(
		"Mapping {} KiB page for task heap ({:#X} => {:#X})",
		S::SIZE >> 10,
		page.address(),
		physical_address
	);

	root_pagetable.map_page(
		page,
		physical_address,
		PageTableEntryFlags::WRITABLE | PageTableEntryFlags::EXECUTE_DISABLE,
	);

	// If our application is a Go application (detected by the presence of the
	// weak symbol "runtime_osinit"), we have to return a zeroed page.
	unsafe {
		if !runtime_osinit.is_null() {
			trace!("Go application detected, returning a zeroed page");
			ptr::write_bytes(page.address() as *mut u8, 0, S::SIZE);
		}
	}

	// clear cr2 to signalize that the pagefault is solved by the pagefault handler
	unsafe {
		controlregs::cr2_write(0);
	}
}

pub extern "x86-interrupt" fn page_fault_handler(
	stack_frame: &mut irq::ExceptionStackFrame,
	error_code: u64,
) {
	let virtual_address = unsafe { controlregs::cr2() };
	let (kernel_heap_start, kernel_heap_end) = ::mm::heap_range();

	if virtual_address >= kernel_heap_start && virtual_address < kernel_heap_end {
		// belongs to the current kernel heap
		map_page_on_demand::<LargePageSize>(virtual_address);
		return;
	} else if let Some(ref heap) = core_scheduler().current_task.borrow().heap {
		// belong to the user space heap
		let heap_borrowed = heap.borrow();
		let heap_locked = heap_borrowed.read();

		// Is the requested virtual address within the boundary of that heap?
		if virtual_address >= heap_locked.start && virtual_address < heap_locked.end {
			map_page_on_demand::<LargePageSize>(virtual_address);
			return;
		}
	}

	// Anything else is an error!
	let pferror = PageFaultError::from_bits_truncate(error_code as u32);
	error!("Page Fault (#PF) Exception: {:#?}", stack_frame);
	error!(
		"virtual_address = {:#X}, page fault error = {}",
		virtual_address, pferror
	);
	error!(
		"fs = {:#X}, gs = {:#X}",
		processor::readfs(),
		processor::readgs()
	);

	// clear cr2 to signalize that the pagefault is solved by the pagefault handler
	unsafe {
		controlregs::cr2_write(0);
	}

	scheduler::abort();
}

#[inline]
fn get_page_range<S: PageSize>(virtual_address: usize, count: usize) -> PageIter<S> {
	let first_page = Page::<S>::including_address(virtual_address);
	let last_page = Page::<S>::including_address(virtual_address + (count - 1) * S::SIZE);
	Page::range(first_page, last_page)
}

pub fn get_page_table_entry<S: PageSize>(virtual_address: usize) -> Option<PageTableEntry> {
	trace!("Looking up Page Table Entry for {:#X}", virtual_address);

	let page = Page::<S>::including_address(virtual_address);
	let root_pagetable = unsafe { &mut *PML4_ADDRESS };
	root_pagetable.get_page_table_entry(page)
}

pub fn get_physical_address<S: PageSize>(virtual_address: usize) -> usize {
	trace!("Getting physical address for {:#X}", virtual_address);

	let page = Page::<S>::including_address(virtual_address);
	let root_pagetable = unsafe { &mut *PML4_ADDRESS };
	let address = root_pagetable
		.get_page_table_entry(page)
		.expect("Entry not present")
		.address();
	let offset = virtual_address & (S::SIZE - 1);
	address | offset
}

/// Translate a virtual memory address to a physical one.
/// Just like get_physical_address, but automatically uses the correct page size for the respective memory address.
pub fn virtual_to_physical(virtual_address: usize) -> usize {
	if virtual_address < mm::kernel_start_address() {
		// Parts of the memory below the kernel image are identity-mapped.
		// However, this range should never be used in a virtual_to_physical call.
		panic!(
			"Trying to get the physical address of {:#X}, which is too low",
			virtual_address
		);
	} else if virtual_address < mm::kernel_end_address() {
		// The kernel image is mapped in 2 MiB pages.
		get_physical_address::<LargePageSize>(virtual_address)
	} else if virtual_address < virtualmem::task_heap_start() {
		// The kernel memory is mapped in 4 KiB pages.
		get_physical_address::<BasePageSize>(virtual_address)
	} else if virtual_address < virtualmem::task_heap_end() {
		// The application memory is mapped in 2 MiB pages.
		get_physical_address::<LargePageSize>(virtual_address)
	} else {
		// This range is currently unused by HermitCore.
		panic!(
			"Trying to get the physical address of {:#X}, which is too high",
			virtual_address
		);
	}
}

#[no_mangle]
pub extern "C" fn virt_to_phys(virtual_address: usize) -> usize {
	virtual_to_physical(virtual_address)
}

pub fn map<S: PageSize>(
	virtual_address: usize,
	physical_address: usize,
	count: usize,
	flags: PageTableEntryFlags,
) {
	trace!(
		"Mapping virtual address {:#X} to physical address {:#X} ({} pages)",
		virtual_address,
		physical_address,
		count
	);

	let range = get_page_range::<S>(virtual_address, count);
	let root_pagetable = unsafe { &mut *PML4_ADDRESS };
	root_pagetable.map_pages(range, physical_address, flags);
}

pub fn identity_map(start_address: usize, end_address: usize) {
	let first_page = Page::<BasePageSize>::including_address(start_address);
	let last_page = Page::<BasePageSize>::including_address(end_address);
	assert!(
		last_page.address() < mm::kernel_start_address(),
		"Address {:#X} to be identity-mapped is not below Kernel start address",
		last_page.address()
	);

	let root_pagetable = unsafe { &mut *PML4_ADDRESS };
	let range = Page::<BasePageSize>::range(first_page, last_page);
	let mut flags = PageTableEntryFlags::empty();
	flags.normal().read_only().execute_disable();
	root_pagetable.map_pages(range, first_page.address(), flags);
}

#[inline]
pub fn get_application_page_size() -> usize {
	LargePageSize::SIZE
}

pub fn init() {}

pub fn init_page_tables() {
	debug!("Create new view to the kernel space");

	unsafe {
		let pml4 = controlregs::cr3();
		let pde = pml4 + 2 * BasePageSize::SIZE as u64;

		debug!("Found PML4 at 0x{:x}", pml4);

		// make sure that only the required areas are mapped
		let start = pde
			+ ((mm::kernel_end_address() >> (PAGE_MAP_BITS + PAGE_BITS)) * mem::size_of::<u64>())
				as u64;
		let size = (512 - (mm::kernel_end_address() >> (PAGE_MAP_BITS + PAGE_BITS)))
			* mem::size_of::<u64>();
		ptr::write_bytes(start as *mut u8, 0, size);

		let size =
			(mm::kernel_start_address() >> (PAGE_MAP_BITS + PAGE_BITS)) * mem::size_of::<u64>();
		ptr::write_bytes(pde as *mut u8, 0, size);

		// flush tlb
		controlregs::cr3_write(pml4);

		if is_uhyve() {
			// we need to map GDT from hypervisor
			identity_map(BOOT_GDT, BOOT_GDT);
		}

		// Identity-map the supplied Multiboot information and command line.
		let mb_info = get_mbinfo();
		if mb_info > 0 {
			identity_map(mb_info, mb_info);

			// Map the "Memory Map" information too.
			let mb = Multiboot::new(mb_info);
			let memory_map_address = mb
				.memory_map_address()
				.expect("Could not find a memory map in the Multiboot information");
			identity_map(memory_map_address, memory_map_address);
		}

		let cmdsize = environment::get_cmdsize();
		if cmdsize > 0 {
			let cmdline = environment::get_cmdline();
			identity_map(cmdline, cmdline + cmdsize - 1);
		}
	}
}
