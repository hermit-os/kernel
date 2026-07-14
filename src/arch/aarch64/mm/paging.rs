use core::arch::asm;
use core::marker::PhantomData;
use core::ptr;

use aarch64_cpu::asm::barrier::{ISH, ISHST, SY, dsb, isb};
use aarch64_cpu::registers::{ID_AA64MMFR0_EL1, Readable};
use align_address::Align;
use free_list::PageLayout;
use memory_addresses::{PhysAddr, VirtAddr};

use crate::mm::{FrameAlloc, PageRangeAllocator};

/// Pointer to the root page table (called "Level 0" in ARM terminology).
/// Setting the upper bits to zero tells the MMU to use TTBR0 for the base address for the first table.
///
/// See entry.S and ARM Cortex-A Series Programmer's Guide for ARMv8-A, Version 1.0, PDF page 172
const L0TABLE_ADDRESS: VirtAddr = VirtAddr::new(0x0000_ffff_ffff_f000u64);

/// Number of Offset bits of a virtual address for a 4 KiB page, which are shifted away to get its Page Frame Number (PFN).
const PAGE_BITS: usize = 12;

/// Number of bits of the index in each table (L0Table, L1Table, L2Table, L3Table).
const PAGE_MAP_BITS: usize = 9;

/// A mask where PAGE_MAP_BITS are set to calculate a table index.
const PAGE_MAP_MASK: usize = 0x1ff;

/// Grouping 4KiB pages to a larger page
const GROUP_SIZE: usize = 16;

bitflags! {
	/// Useful flags for an entry in either table (L0Table, L1Table, L2Table, L3Table).
	///
	/// See ARM Architecture Reference Manual, ARMv8, for ARMv8-A Reference Profile, Issue C.a, Chapter D4.3.3
	#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
	pub struct PageTableEntryFlags: u64 {
		/// Set if this entry is valid.
		const PRESENT = 1 << 0;

		/// Set if this entry points to a table or a 4 KiB page.
		const TABLE_OR_4KIB_PAGE = 1 << 1;

		/// Set if this entry points to device memory (non-gathering, non-reordering, no early write acknowledgement)
		const DEVICE_NGNRNE = 0;

		/// Set if this entry points to device memory (non-gathering, non-reordering, early write acknowledgement)
		const DEVICE_NGNRE = 1 << 2;

		/// Set if this entry points to device memory (gathering, reordering, early write acknowledgement)
		const DEVICE_GRE = 1 << 3;

		/// Set if this entry points to normal memory (non-cacheable)
		const NORMAL_NC = (1 << 3) | (1 << 2);

		/// Set if this entry points to normal memory (cacheable)
		const NORMAL = 1 << 4;

		/// Set if memory referenced by this entry shall also be accessible from EL0 (user mode).
		const USER_ACCESSIBLE = 1 << 6;

		/// Set if memory referenced by this entry shall be read-only.
		const READ_ONLY = 1 << 7;

		/// Set if this entry shall be shared between all cores of the system.
		const INNER_SHAREABLE = (1 << 8) | (1 << 9);

		/// Set if software has accessed this entry (for memory access or address translation).
		const ACCESSED = 1 << 10;

		/// Translation table contiguous  to the previous for initial lookup
		const CONTIGUOUS = 1 << 52;

		/// Set if code execution shall be disabled for memory referenced by this entry in privileged mode.
		const PRIVILEGED_EXECUTE_NEVER = 1 << 53;

		/// Set if code execution shall be disabled for memory referenced by this entry in unprivileged mode.
		const UNPRIVILEGED_EXECUTE_NEVER = 1 << 54;

		/// Self-reference to the Level 0 page table
		const SELF = 1 << 55;
	}
}

impl PageTableEntryFlags {
	#[allow(dead_code)]
	pub fn present(&mut self) -> &mut Self {
		self.insert(PageTableEntryFlags::PRESENT);
		self
	}

	pub fn device(&mut self) -> &mut Self {
		self.remove(PageTableEntryFlags::NORMAL);
		self.remove(PageTableEntryFlags::NORMAL_NC);
		self.remove(PageTableEntryFlags::DEVICE_NGNRE);
		self.remove(PageTableEntryFlags::DEVICE_GRE);
		self.insert(PageTableEntryFlags::DEVICE_NGNRNE);
		self
	}

	pub fn normal(&mut self) -> &mut Self {
		self.remove(PageTableEntryFlags::NORMAL_NC);
		self.remove(PageTableEntryFlags::DEVICE_NGNRE);
		self.remove(PageTableEntryFlags::DEVICE_GRE);
		self.insert(PageTableEntryFlags::NORMAL);
		self
	}

	#[allow(dead_code)]
	pub fn read_only(&mut self) -> &mut Self {
		self.insert(PageTableEntryFlags::READ_ONLY);
		self
	}

	pub fn writable(&mut self) -> &mut Self {
		self.remove(PageTableEntryFlags::READ_ONLY);
		self
	}

	pub fn execute_disable(&mut self) -> &mut Self {
		self.insert(PageTableEntryFlags::PRIVILEGED_EXECUTE_NEVER);
		self.insert(PageTableEntryFlags::UNPRIVILEGED_EXECUTE_NEVER);
		self
	}

	#[cfg(feature = "common-os")]
	pub fn execute_enable(&mut self) -> &mut Self {
		self.remove(PageTableEntryFlags::PRIVILEGED_EXECUTE_NEVER);
		self.remove(PageTableEntryFlags::UNPRIVILEGED_EXECUTE_NEVER);
		self
	}

	#[cfg(feature = "common-os")]
	pub fn user(&mut self) -> &mut Self {
		self.insert(PageTableEntryFlags::USER_ACCESSIBLE);
		// An EL0-accessible page must not be executable from EL1.
		self.insert(PageTableEntryFlags::PRIVILEGED_EXECUTE_NEVER);
		self
	}

	#[cfg(feature = "common-os")]
	pub fn kernel(&mut self) -> &mut Self {
		self.remove(PageTableEntryFlags::USER_ACCESSIBLE);
		self
	}
}

/// Extension trait that mirrors the x86_64 `PageTableEntryFlagsExt` API for the
/// common-os layer. On AArch64 the `PageTableEntryFlags` type already carries
/// the same helpers as inherent methods; this trait exists only so that
/// architecture-independent callers can be written in terms of a single API.
#[cfg(feature = "common-os")]
#[allow(dead_code)]
pub trait PageTableEntryFlagsExt {
	fn device(&mut self) -> &mut Self;
	fn normal(&mut self) -> &mut Self;
	fn read_only(&mut self) -> &mut Self;
	fn writable(&mut self) -> &mut Self;
	fn execute_disable(&mut self) -> &mut Self;
	fn execute_enable(&mut self) -> &mut Self;
	fn user(&mut self) -> &mut Self;
	fn kernel(&mut self) -> &mut Self;
}

#[cfg(feature = "common-os")]
impl PageTableEntryFlagsExt for PageTableEntryFlags {
	fn device(&mut self) -> &mut Self {
		PageTableEntryFlags::device(self)
	}
	fn normal(&mut self) -> &mut Self {
		PageTableEntryFlags::normal(self)
	}
	fn read_only(&mut self) -> &mut Self {
		PageTableEntryFlags::read_only(self)
	}
	fn writable(&mut self) -> &mut Self {
		PageTableEntryFlags::writable(self)
	}
	fn execute_disable(&mut self) -> &mut Self {
		PageTableEntryFlags::execute_disable(self)
	}
	fn execute_enable(&mut self) -> &mut Self {
		PageTableEntryFlags::execute_enable(self)
	}
	fn user(&mut self) -> &mut Self {
		PageTableEntryFlags::user(self)
	}
	fn kernel(&mut self) -> &mut Self {
		PageTableEntryFlags::kernel(self)
	}
}

/// An entry in either table
#[derive(Clone, Copy, Default, Debug)]
pub struct PageTableEntry {
	/// Physical memory address this entry refers, combined with flags from PageTableEntryFlags.
	physical_address_and_flags: u64,
}

impl PageTableEntry {
	/// Return the stored physical address.
	pub fn address(&self) -> PhysAddr {
		PhysAddr::new_truncate(
			self.physical_address_and_flags & !(BasePageSize::SIZE - 1u64) & !(u64::MAX << 48),
		)
	}

	/// Returns whether this entry is valid (present).
	fn is_present(&self) -> bool {
		(self.physical_address_and_flags & PageTableEntryFlags::PRESENT.bits()) != 0
	}

	/// Return whether this entry is a 4KiB page.
	#[cfg(feature = "common-os")]
	fn is_table_or_4kib_page(&self) -> bool {
		(self.physical_address_and_flags & PageTableEntryFlags::TABLE_OR_4KIB_PAGE.bits()) != 0
	}

	/// Mark this as a valid (present) entry and set address translation and flags.
	///
	/// # Arguments
	///
	/// * `physical_address` - The physical memory address this entry shall translate to
	/// * `flags` - Flags from PageTableEntryFlags (note that the PRESENT, INNER_SHAREABLE, and ACCESSED flags are set automatically)
	fn set(&mut self, physical_address: PhysAddr, flags: PageTableEntryFlags) {
		// Verify that the offset bits for a 4 KiB page are zero.
		assert!(
			physical_address.is_aligned_to(BasePageSize::SIZE),
			"Physical address is not on a 4 KiB page boundary (physical_address = {physical_address:p})"
		);

		let mut flags_to_set = flags;
		flags_to_set.insert(PageTableEntryFlags::PRESENT);
		flags_to_set.insert(PageTableEntryFlags::INNER_SHAREABLE);
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
	const SIZE: u64;

	/// The page table level at which a page of this size is mapped
	const MAP_LEVEL: usize;

	/// Any extra flag that needs to be set to map a page of this size.
	/// For example: PageTableEntryFlags::TABLE_OR_4KIB_PAGE.
	const MAP_EXTRA_FLAG: PageTableEntryFlags;
}

/// A 4 KiB page mapped in the L3Table.
#[derive(Clone, Copy)]
pub enum BasePageSize {}
impl PageSize for BasePageSize {
	const SIZE: u64 = 4096;
	const MAP_LEVEL: usize = 3;
	const MAP_EXTRA_FLAG: PageTableEntryFlags = PageTableEntryFlags::TABLE_OR_4KIB_PAGE;
}

/// A 2 MiB page mapped in the L2Table.
#[derive(Clone, Copy)]
pub enum LargePageSize {}
impl PageSize for LargePageSize {
	const SIZE: u64 = 2 * 1024 * 1024;
	const MAP_LEVEL: usize = 2;
	const MAP_EXTRA_FLAG: PageTableEntryFlags = PageTableEntryFlags::empty();
}

/// A 1 GiB page mapped in the L1Table.
#[derive(Clone, Copy)]
pub enum HugePageSize {}
impl PageSize for HugePageSize {
	const SIZE: u64 = 1024 * 1024 * 1024;
	const MAP_LEVEL: usize = 1;
	const MAP_EXTRA_FLAG: PageTableEntryFlags = PageTableEntryFlags::empty();
}

/// A memory page of the size given by S.
#[derive(Clone, Copy)]
struct Page<S: PageSize> {
	/// Virtual memory address of this page.
	/// This is rounded to a page size boundary on creation.
	virtual_address: VirtAddr,

	/// Required by Rust to support the S parameter.
	size: PhantomData<S>,
}

impl<S: PageSize> Page<S> {
	/// Return the stored virtual address.
	#[expect(dead_code)]
	fn address(&self) -> VirtAddr {
		self.virtual_address
	}

	/// Flushes this page from the TLB of this CPU.
	fn flush_from_tlb(&self) {
		// See ARM Cortex-A Series Programmer's Guide for ARMv8-A, Version 1.0, PDF page 198
		//
		// We use "vale1is" instead of "vae1is" to always flush the last table level only (performance optimization).
		// The "is" attribute broadcasts the TLB flush to all cores, so we don't need an IPI (unlike x86_64).
		dsb(ISHST);
		unsafe {
			asm!(
				"tlbi vale1is, {addr}",
				addr = in(reg) self.virtual_address.as_u64() >> 12,
				options(nostack),
			);
		}
		dsb(ISH);
		isb(SY);
	}

	/// Returns whether the given virtual address is a valid one in the AArch64 memory model.
	///
	/// Current AArch64 supports only 48-bit for virtual memory addresses.
	/// The upper bits must always be 0 or 1 and indicate whether TBBR0 or TBBR1 contains the
	/// base address. So always enforce 0 here.
	fn is_valid_address(virtual_address: VirtAddr) -> bool {
		virtual_address < VirtAddr::new(0x1_0000_0000_0000)
	}

	/// Returns a Page including the given virtual address.
	/// That means, the address is rounded down to a page size boundary.
	fn including_address(virtual_address: VirtAddr) -> Self {
		assert!(
			Self::is_valid_address(virtual_address),
			"Virtual address {virtual_address:p} is invalid"
		);

		Self {
			virtual_address: virtual_address.align_down(S::SIZE),
			size: PhantomData,
		}
	}

	/// Returns a PageIter to iterate from the given first Page to the given last Page (inclusive).
	fn range(first: Self, last: Self) -> PageIter<S> {
		assert!(first.virtual_address <= last.virtual_address);
		PageIter {
			current: first,
			last,
		}
	}

	/// Returns the index of this page in the table given by L.
	fn table_index<L: PageTableLevel>(&self) -> usize {
		assert!(L::LEVEL <= S::MAP_LEVEL);
		(self.virtual_address.as_usize() >> PAGE_BITS >> ((3 - L::LEVEL) * PAGE_MAP_BITS))
			& PAGE_MAP_MASK
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
		if self.last.virtual_address < self.current.virtual_address {
			return None;
		}

		let p = self.current;
		self.current.virtual_address += S::SIZE;
		Some(p)
	}
}

/// An interface to allow for a generic implementation of struct PageTable for all 4 page tables.
/// Must be implemented by all page tables.
trait PageTableLevel {
	/// Numeric page table level
	const LEVEL: usize;
}

/// An interface for page tables with sub page tables (all except L3Table).
/// Having both PageTableLevel and PageTableLevelWithSubtables leverages Rust's typing system to provide
/// a subtable method only for those that have sub page tables.
///
/// Kudos to Philipp Oppermann for the trick!
trait PageTableLevelWithSubtables: PageTableLevel {
	type SubtableLevel;
}

/// The Level 0 Table
enum L0Table {}
impl PageTableLevel for L0Table {
	const LEVEL: usize = 0;
}

impl PageTableLevelWithSubtables for L0Table {
	type SubtableLevel = L1Table;
}

/// The Level 1 Table (can map 1 GiB pages)
enum L1Table {}
impl PageTableLevel for L1Table {
	const LEVEL: usize = 1;
}

impl PageTableLevelWithSubtables for L1Table {
	type SubtableLevel = L2Table;
}

/// The Level 2 Table (can map 2 MiB pages)
enum L2Table {}
impl PageTableLevel for L2Table {
	const LEVEL: usize = 2;
}

impl PageTableLevelWithSubtables for L2Table {
	type SubtableLevel = L3Table;
}

/// The Level 3 Table (can map 4 KiB pages)
enum L3Table {}
impl PageTableLevel for L3Table {
	const LEVEL: usize = 3;
}

/// Representation of any page table in memory.
/// Parameter L supplies information for Rust's typing system to distinguish between the different tables.
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
		physical_address: PhysAddr,
		flags: PageTableEntryFlags,
	);
	fn map_page<S: PageSize>(
		&mut self,
		page: Page<S>,
		physical_address: PhysAddr,
		flags: PageTableEntryFlags,
	);
}

impl<L: PageTableLevel> PageTableMethods for PageTable<L> {
	/// Maps a single page in this table to the given physical address.
	///
	/// Must only be called if a page of this size is mapped at this page table level!
	fn map_page_in_this_table<S: PageSize>(
		&mut self,
		page: Page<S>,
		physical_address: PhysAddr,
		flags: PageTableEntryFlags,
	) {
		assert_eq!(L::LEVEL, S::MAP_LEVEL);
		let index = page.table_index::<L>();
		let flush = self.entries[index].is_present();

		if flush {
			// The reference manual suggests to replace the entry with an invalid entry first
			// and then with the entry we actually want to see. And on top of that the procedure
			// is heavily reinforced with memory barrier instructions along the way.
			self.entries[index] = PageTableEntry::default();
			page.flush_from_tlb();
		}

		if flags == PageTableEntryFlags::empty() {
			// We already unmapped the page
			return;
		} else {
			self.entries[index].set(physical_address, S::MAP_EXTRA_FLAG | flags);
		}

		if flush {
			page.flush_from_tlb();
		}
	}

	/// Returns the PageTableEntry for the given page if it is present, otherwise returns None.
	///
	/// This is the default implementation called only for L3Table.
	/// It is overridden by a specialized implementation for all tables with sub tables (all except L3Table).
	default fn get_page_table_entry<S: PageSize>(&self, page: Page<S>) -> Option<PageTableEntry> {
		assert_eq!(L::LEVEL, S::MAP_LEVEL);
		let index = page.table_index::<L>();

		if !self.entries[index].is_present() {
			return None;
		}

		Some(self.entries[index])
	}

	/// Maps a single page to the given physical address.
	///
	/// This is the default implementation that just calls the map_page_in_this_table method.
	/// It is overridden by a specialized implementation for all tables with sub tables (all except L3Table).
	default fn map_page<S: PageSize>(
		&mut self,
		page: Page<S>,
		physical_address: PhysAddr,
		flags: PageTableEntryFlags,
	) {
		self.map_page_in_this_table::<S>(page, physical_address, flags);
	}
}

impl<L: PageTableLevelWithSubtables> PageTableMethods for PageTable<L>
where
	L::SubtableLevel: PageTableLevel,
{
	/// Returns the PageTableEntry for the given page if it is present, otherwise returns None.
	///
	/// This is the implementation for all tables with subtables (L0Table, L1Table, L2Table).
	/// It overrides the default implementation above.
	fn get_page_table_entry<S: PageSize>(&self, page: Page<S>) -> Option<PageTableEntry> {
		assert!(L::LEVEL <= S::MAP_LEVEL);
		let index = page.table_index::<L>();

		if !self.entries[index].is_present() {
			return None;
		}

		if L::LEVEL < S::MAP_LEVEL {
			let subtable = self.subtable::<S>(page);
			subtable.get_page_table_entry::<S>(page)
		} else {
			Some(self.entries[index])
		}
	}

	/// Maps a single page to the given physical address.
	///
	/// This is the implementation for all tables with subtables (L0Table, L1Table, L2Table).
	/// It overrides the default implementation above.
	fn map_page<S: PageSize>(
		&mut self,
		page: Page<S>,
		physical_address: PhysAddr,
		flags: PageTableEntryFlags,
	) {
		assert!(L::LEVEL <= S::MAP_LEVEL);

		if L::LEVEL < S::MAP_LEVEL {
			let index = page.table_index::<L>();

			// Does the table exist yet?
			if !self.entries[index].is_present() {
				// Allocate a single 4 KiB page for the new entry and mark it as a valid, writable subtable.
				let frame_layout = PageLayout::from_size(BasePageSize::SIZE as usize).unwrap();
				let frame_range =
					FrameAlloc::allocate(frame_layout).expect("Unable to allocate physical memory");
				let physical_address = PhysAddr::from(frame_range.start());
				self.entries[index].set(
					physical_address,
					PageTableEntryFlags::NORMAL | PageTableEntryFlags::TABLE_OR_4KIB_PAGE,
				);

				// On a M1 processor is tlb flush required. Otherwise, a page fault sometime occurs.
				// Memory barriers isn't enough to avoid this issue.
				page.flush_from_tlb();

				// Mark all entries as unused in the newly created table.
				let subtable = self.subtable::<S>(page);
				subtable.entries.fill(PageTableEntry::default());
			}

			let subtable = self.subtable::<S>(page);
			subtable.map_page::<S>(page, physical_address, flags);
		} else {
			// Calling the default implementation from a specialized one is not supported (yet),
			// so we have to resort to an extra function.
			self.map_page_in_this_table::<S>(page, physical_address, flags);
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
	// FIXME: https://github.com/hermit-os/kernel/issues/771
	#[allow(clippy::mut_from_ref)]
	fn subtable<S: PageSize>(&self, page: Page<S>) -> &mut PageTable<L::SubtableLevel> {
		assert!(L::LEVEL < S::MAP_LEVEL);

		// Calculate the address of the subtable.
		let index = page.table_index::<L>();
		let table_address = ptr::from_ref(self).addr();
		let subtable_address =
			(table_address << PAGE_MAP_BITS) & !(usize::MAX << 48) | (index << PAGE_BITS);
		unsafe { &mut *(ptr::with_exposed_provenance_mut(subtable_address)) }
	}

	/// Maps a continuous range of pages.
	///
	/// # Arguments
	///
	/// * `range` - The range of pages of size S
	/// * `physical_address` - First physical address to map these pages to
	/// * `flags` - Flags from PageTableEntryFlags to set for the page table entry (e.g. WRITABLE or NO_EXECUTE).
	///   The PRESENT and ACCESSED are already set automatically.
	fn map_pages<S: PageSize>(
		&mut self,
		range: PageIter<S>,
		physical_address: PhysAddr,
		flags: PageTableEntryFlags,
	) {
		let mut current_physical_address = physical_address;

		for page in range {
			self.map_page::<S>(page, current_physical_address, flags);
			current_physical_address += S::SIZE;
		}
	}
}

#[inline]
fn get_page_range<S: PageSize>(virtual_address: VirtAddr, count: usize) -> PageIter<S> {
	let first_page = Page::<S>::including_address(virtual_address);
	let last_page = Page::<S>::including_address(virtual_address + (count as u64 - 1) * S::SIZE);
	Page::range(first_page, last_page)
}

fn get_physical_address<S: PageSize>(virtual_address: VirtAddr) -> Option<PhysAddr> {
	trace!("Getting physical address for {virtual_address:p}");

	let page = Page::<S>::including_address(virtual_address);
	let root_pagetable = unsafe { &mut *(L0TABLE_ADDRESS.as_mut_ptr::<PageTable<L0Table>>()) };
	let address = root_pagetable.get_page_table_entry(page)?.address();
	let offset = virtual_address & (S::SIZE - 1);
	Some(PhysAddr::new(address | offset))
}

/// Translate a virtual memory address to a physical one.
/// Just like get_physical_address, but automatically uses the correct page size for the respective memory address.
pub fn virtual_to_physical(virtual_address: VirtAddr) -> Option<PhysAddr> {
	// Currently, we use only 4K pages.
	get_physical_address::<BasePageSize>(virtual_address)
}

pub fn map<S: PageSize>(
	mut virtual_address: VirtAddr,
	mut physical_address: PhysAddr,
	mut count: usize,
	flags: PageTableEntryFlags,
) {
	trace!(
		"Mapping virtual address {virtual_address:p} to physical address {physical_address:p} ({count} pages)"
	);

	if count < GROUP_SIZE {
		let range = get_page_range::<S>(virtual_address, count);
		let root_pagetable = unsafe { &mut *(L0TABLE_ADDRESS.as_mut_ptr::<PageTable<L0Table>>()) };
		root_pagetable.map_pages(range, physical_address, flags);
	} else {
		// map to GROUP_SIZE*S:SIZE boundary
		let offset = virtual_address.as_usize() % (GROUP_SIZE * S::SIZE as usize);
		if offset > 0 {
			map::<S>(
				virtual_address,
				physical_address,
				offset / S::SIZE as usize,
				flags,
			);
			virtual_address += offset;
			physical_address += offset;
			count -= offset / S::SIZE as usize;
		}

		// map with contiguous bit
		if count >= GROUP_SIZE {
			let map_pages = count.align_down(GROUP_SIZE);
			let range = get_page_range::<S>(virtual_address, map_pages);
			let root_pagetable =
				unsafe { &mut *(L0TABLE_ADDRESS.as_mut_ptr::<PageTable<L0Table>>()) };

			trace!("Mapping {map_pages} pages with contiguous bit");
			root_pagetable.map_pages(
				range,
				physical_address,
				flags | PageTableEntryFlags::CONTIGUOUS,
			);
			virtual_address += map_pages * S::SIZE as usize;
			physical_address += map_pages * S::SIZE as usize;
			count -= map_pages;
		}

		// map the remaining pages
		if count > 0 {
			map::<S>(virtual_address, physical_address, count, flags);
		}
	}
}

/// Maps `nr_pages` pages at address `virt_addr`. If the allocation of a physical memory failed,
/// the number of successful mapped pages are returned as error value.
pub fn map_heap<S: PageSize>(virt_addr: VirtAddr, nr_pages: usize) -> Result<(), usize> {
	let flags = {
		let mut flags = PageTableEntryFlags::empty();
		flags.normal().writable().execute_disable();
		flags
	};
	let mut map_counter = 0;

	while map_counter < nr_pages {
		let size = (nr_pages - map_counter) * S::SIZE as usize;
		for i in (S::SIZE as usize..=size).rev().step_by(S::SIZE as usize) {
			let layout = PageLayout::from_size_align(i, S::SIZE as usize).unwrap();
			let frame_range = FrameAlloc::allocate(layout);

			if let Ok(frame_range) = frame_range {
				let phys_addr = PhysAddr::from(frame_range.start());
				map::<S>(
					virt_addr + map_counter * S::SIZE as usize,
					phys_addr,
					i / S::SIZE as usize,
					flags,
				);
				map_counter += i / S::SIZE as usize;
				break;
			}
		}
	}

	if map_counter < nr_pages {
		return Err(map_counter);
	}

	Ok(())
}

pub fn identity_map<S: PageSize>(phys_addr: PhysAddr) {
	let virt_addr = VirtAddr::new(phys_addr.as_u64());
	let flags = {
		let mut flags = PageTableEntryFlags::empty();
		flags.normal().writable().execute_disable();
		flags
	};

	map::<S>(virt_addr, phys_addr, 1, flags);
}

pub fn unmap<S: PageSize>(virtual_address: VirtAddr, count: usize) {
	trace!("Unmapping virtual address {virtual_address:p} ({count} pages)");

	let range = get_page_range::<S>(virtual_address, count);
	let root_pagetable = unsafe { &mut *(L0TABLE_ADDRESS.as_mut_ptr::<PageTable<L0Table>>()) };
	root_pagetable.map_pages(range, PhysAddr::zero(), PageTableEntryFlags::empty());
}

/// Flush the entire (non-global) TLB on this core and broadcast to others.
#[cfg(feature = "common-os")]
fn flush_tlb_all() {
	dsb(ISHST);
	unsafe {
		asm!("tlbi vmalle1is", options(nostack));
	}
	dsb(ISH);
	isb(SY);
}

/// Recursively free the user-space portion of a given L0 page table.
///
/// Entry 0 (kernel low mapping) and entry 511 (self-reference) are preserved.
///
/// **Important**: Hermit shares TTBR0_EL1 between user space and the kernel's
/// own heap and task stacks (PageAlloc hands out addresses in the high half of
/// the low 48-bit range, e.g. starting at `0x800000000000`). The same L0
/// entries (1..510) can therefore back **mixed** kernel + user mappings — and
/// even L1/L2/L3 tables further down might contain a mix.
///
/// We must only free a sub-table once it is **completely empty** after the
/// user-page sweep; otherwise we would unmap the kernel's own heap or the
/// kernel stack the current task is running on, which makes the very next
/// stack access fault and the CPU spins in recursive sync exceptions.
#[cfg(feature = "common-os")]
fn clear_l0(l0_phys: usize) {
	fn free_frame(phys: usize) {
		let range = free_list::PageRange::new(phys, phys + BasePageSize::SIZE as usize).unwrap();
		unsafe { FrameAlloc::deallocate(range) };
	}

	fn pt_is_empty<L>(pt: &PageTable<L>) -> bool
	where
		L: PageTableLevel,
	{
		pt.entries.iter().all(|e| !e.is_present())
	}

	let l0 = unsafe { &mut *ptr::with_exposed_provenance_mut::<PageTable<L0Table>>(l0_phys) };

	// Only walk the user-space L0 slot. All other entries point at
	// kernel L1 tables that are *shared* across every task's PT — clearing
	// them here would unmap the kernel's own heap and stack.
	for l0_idx in [USER_L0_INDEX].iter().copied() {
		let l0_entry = &mut l0.entries[l0_idx];
		if !l0_entry.is_present() {
			continue;
		}
		let l1_phys = l0_entry.address().as_usize();
		let l1 = unsafe { &mut *ptr::with_exposed_provenance_mut::<PageTable<L1Table>>(l1_phys) };

		for l1_idx in 0..512usize {
			let l1_entry = &mut l1.entries[l1_idx];
			if !l1_entry.is_present() {
				continue;
			}
			if !l1_entry.is_table_or_4kib_page() {
				warn!("User space isn't able to use 1 GiB pages");
				continue;
			}
			let l2_phys = l1_entry.address().as_usize();
			let l2 =
				unsafe { &mut *ptr::with_exposed_provenance_mut::<PageTable<L2Table>>(l2_phys) };

			for l2_idx in 0..512usize {
				let l2_entry = &mut l2.entries[l2_idx];
				if !l2_entry.is_present() {
					continue;
				}
				if !l2_entry.is_table_or_4kib_page() {
					warn!("User space isn't able to use 2 MiB pages");
					continue;
				}
				let l3_phys = l2_entry.address().as_usize();
				let l3 = unsafe {
					&mut *ptr::with_exposed_provenance_mut::<PageTable<L3Table>>(l3_phys)
				};

				for l3_idx in 0..512usize {
					let l3_entry = &mut l3.entries[l3_idx];
					let flags = PageTableEntryFlags::from_bits_truncate(
						l3_entry.physical_address_and_flags,
					);
					if flags.contains(PageTableEntryFlags::PRESENT)
						&& flags.contains(PageTableEntryFlags::USER_ACCESSIBLE)
					{
						let phys_addr = l3_entry.address();

						{
							free_frame(phys_addr.as_usize());
						}
						*l3_entry = PageTableEntry::default();
					}
				}

				if pt_is_empty(l3) {
					free_frame(l3_phys);
					*l2_entry = PageTableEntry::default();
				}
			}

			if pt_is_empty(l2) {
				free_frame(l2_phys);
				*l1_entry = PageTableEntry::default();
			}
		}

		if pt_is_empty(l1) {
			free_frame(l1_phys);
			*l0_entry = PageTableEntry::default();
		}
	}
}

/// Drop an inactive user address space: free all user pages and all user-space
/// page-table pages, then free the L0 table itself.
#[cfg(feature = "common-os")]
pub fn drop_user_space(l0_phys: usize) {
	debug!("Drop the user space at L0 {l0_phys:#x}");

	clear_l0(l0_phys);

	// The L0 table is not loaded on any core, so no TLB flush is necessary.
	let range = free_list::PageRange::new(l0_phys, l0_phys + BasePageSize::SIZE as usize).unwrap();
	unsafe { FrameAlloc::deallocate(range) };
}

/// Clear the user-space portion of the currently active address space.
#[cfg(feature = "common-os")]
pub fn clear_user_space() {
	use aarch64_cpu::registers::TTBR0_EL1;

	use crate::core_scheduler;
	use crate::fd::STDERR_FILENO;

	core_scheduler()
		.get_current_task()
		.borrow()
		.vmas
		.write()
		.clear();
	core_scheduler()
		.get_current_task_object_map()
		.write()
		.retain(|&k, _| k <= STDERR_FILENO);

	let l0_phys = TTBR0_EL1.get_baddr() as usize;
	debug!("Clear the user space at L0 {l0_phys:#x}");

	clear_l0(l0_phys);

	flush_tlb_all();
}

/// Allocate a fresh L0 (root) page table for a new address space.
///
/// The new L0 inherits the kernel low mapping from the currently active L0
/// (entry 0) and installs a self-reference at entry 511. User-space entries
/// (1..511) are left empty.
/// Index of the L0 entry that backs the user-space load area
/// (`USER_START = 0x0100_0000_0000`, bits 47..39 = 2).
#[cfg(feature = "common-os")]
const USER_L0_INDEX: usize = (crate::arch::aarch64::kernel::USER_START.as_usize() >> 39) & 0x1ff;

#[cfg(feature = "common-os")]
pub fn create_new_root_page_table() -> usize {
	use aarch64_cpu::registers::TTBR0_EL1;

	let layout = PageLayout::from_size(BasePageSize::SIZE as usize).unwrap();
	let frame_range = FrameAlloc::allocate(layout).expect("Failed to allocate L0 table");
	let new_l0_phys = frame_range.start();

	let cur_l0_phys = TTBR0_EL1.get_baddr() as usize;
	let cur_l0 = unsafe { &*ptr::with_exposed_provenance::<PageTable<L0Table>>(cur_l0_phys) };

	let new_l0 =
		unsafe { &mut *ptr::with_exposed_provenance_mut::<PageTable<L0Table>>(new_l0_phys) };

	// Inherit every L0 entry from the current (kernel) page table EXCEPT
	// the user-space slot — that one is left empty so the new task starts
	// with a clean user address space.
	//
	// Sharing the kernel L0 entries means we share the L1/L2/L3 tables
	// underneath. That is intentional: kernel mappings (kernel image,
	// heap, per-CPU stacks, …) are global and must
	// stay in sync across every common-os task. Without sharing, the
	// kernel would lose access to its own heap as soon as the scheduler
	// switches TTBR0_EL1 to this task's PT.
	for (i, entry) in new_l0.entries.iter_mut().enumerate() {
		*entry = if i == USER_L0_INDEX || i == 511 {
			PageTableEntry::default()
		} else {
			cur_l0.entries[i]
		};
	}

	let self_flags = PageTableEntryFlags::PRESENT
		| PageTableEntryFlags::TABLE_OR_4KIB_PAGE
		| PageTableEntryFlags::NORMAL
		| PageTableEntryFlags::INNER_SHAREABLE
		| PageTableEntryFlags::ACCESSED
		| PageTableEntryFlags::SELF;
	new_l0.entries[511].set(PhysAddr::new(new_l0_phys as u64), self_flags);

	new_l0_phys
}

/// Returns the physical address of the current task's root page table.
#[allow(dead_code)]
#[cfg(feature = "common-os")]
pub fn get_current_root_page_table() -> usize {
	use crate::arch::kernel::core_local::core_scheduler;
	core_scheduler()
		.get_current_task()
		.borrow()
		.root_page_table
		.as_usize()
}

pub unsafe fn init() {
	// determine physical address size
	let id_aa64mmfr0_el1 = ID_AA64MMFR0_EL1.extract();

	let pa_range = match id_aa64mmfr0_el1
		.read_as_enum(ID_AA64MMFR0_EL1::PARange)
		.unwrap()
	{
		ID_AA64MMFR0_EL1::PARange::Value::Bits_32 => 32,
		ID_AA64MMFR0_EL1::PARange::Value::Bits_36 => 36,
		ID_AA64MMFR0_EL1::PARange::Value::Bits_40 => 40,
		ID_AA64MMFR0_EL1::PARange::Value::Bits_42 => 42,
		ID_AA64MMFR0_EL1::PARange::Value::Bits_44 => 44,
		ID_AA64MMFR0_EL1::PARange::Value::Bits_48 => 48,
		ID_AA64MMFR0_EL1::PARange::Value::Bits_52 => 52,
	};

	info!("Physical address range: {}GB", 1 << (pa_range - 30));

	let t_gran4 = id_aa64mmfr0_el1.matches_all(ID_AA64MMFR0_EL1::TGran4::Supported);
	let t_gran64 = id_aa64mmfr0_el1.matches_all(ID_AA64MMFR0_EL1::TGran64::Supported);
	let t_gran16 = id_aa64mmfr0_el1.matches_all(ID_AA64MMFR0_EL1::TGran16::Supported);

	info!("Support of 4KB pages: {t_gran4}");
	info!("Support of 16KB pages: {t_gran16}");
	info!("Support of 64KB pages: {t_gran64}");

	assert!(t_gran4);

	// page tables are already initialized, we have just to remove obsolete entries
}
