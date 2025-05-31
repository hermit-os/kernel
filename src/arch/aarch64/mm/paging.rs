#![allow(unused)]

use core::arch::asm;
use core::marker::PhantomData;
use core::{fmt, mem, ptr};

use align_address::Align;
use memory_addresses::{PhysAddr, VirtAddr};

use crate::arch::aarch64::kernel::{get_base_address, get_image_size, get_ram_address, processor};
use crate::env::is_uhyve;
use crate::mm::physicalmem;
use crate::{KERNEL_STACK_SIZE, mm, scheduler};

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
	/// An empty set of flags for unused/zeroed table entries.
	/// Needed as long as empty() is no const function.
	const BLANK: PageTableEntryFlags = PageTableEntryFlags::empty();

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
	const MAP_EXTRA_FLAG: PageTableEntryFlags = PageTableEntryFlags::BLANK;
}

/// A 1 GiB page mapped in the L1Table.
#[derive(Clone, Copy)]
pub enum HugePageSize {}
impl PageSize for HugePageSize {
	const SIZE: u64 = 1024 * 1024 * 1024;
	const MAP_LEVEL: usize = 1;
	const MAP_EXTRA_FLAG: PageTableEntryFlags = PageTableEntryFlags::BLANK;
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
	fn address(&self) -> VirtAddr {
		self.virtual_address
	}

	/// Flushes this page from the TLB of this CPU.
	fn flush_from_tlb(&self) {
		// See ARM Cortex-A Series Programmer's Guide for ARMv8-A, Version 1.0, PDF page 198
		//
		// We use "vale1is" instead of "vae1is" to always flush the last table level only (performance optimization).
		// The "is" attribute broadcasts the TLB flush to all cores, so we don't need an IPI (unlike x86_64).
		unsafe {
			asm!(
				"dsb ishst",
				"tlbi vale1is, {addr}",
				"dsb ish",
				"isb",
				addr = in(reg) self.virtual_address.as_u64() >> 12,
				options(nostack),
			);
		}
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

		if flags == PageTableEntryFlags::BLANK {
			// in this case we unmap the pages
			self.entries[index].set(physical_address, flags);
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

		if self.entries[index].is_present() {
			Some(self.entries[index])
		} else {
			None
		}
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

		if self.entries[index].is_present() {
			if L::LEVEL < S::MAP_LEVEL {
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
				let physical_address = physicalmem::allocate(BasePageSize::SIZE as usize)
					.expect("Unable to allocate physical memory");
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
		let table_address = core::ptr::from_ref(self).addr();
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

pub fn get_page_table_entry<S: PageSize>(virtual_address: VirtAddr) -> Option<PageTableEntry> {
	trace!("Looking up Page Table Entry for {virtual_address:p}");

	let page = Page::<S>::including_address(virtual_address);
	let root_pagetable = unsafe { &mut *(L0TABLE_ADDRESS.as_mut_ptr::<PageTable<L0Table>>()) };
	root_pagetable.get_page_table_entry(page)
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
			if let Ok(phys_addr) = physicalmem::allocate_aligned(i, S::SIZE as usize) {
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
		Err(map_counter)
	} else {
		Ok(())
	}
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
	root_pagetable.map_pages(range, PhysAddr::zero(), PageTableEntryFlags::BLANK);
}

#[inline]
pub fn get_application_page_size() -> usize {
	BasePageSize::SIZE as usize
}

pub unsafe fn init() {
	let aa64mmfr0: u64;

	let ram_start = get_ram_address();
	info!("RAM starts at physical address {ram_start:p}");

	// determine physical address size
	unsafe {
		asm!(
			"mrs {}, id_aa64mmfr0_el1",
			out(reg) aa64mmfr0,
			options(nostack),
		);
	}

	let pa_range: u64 = match aa64mmfr0 & 0b1111 {
		0b0000 => 32,
		0b0001 => 36,
		0b0010 => 40,
		0b0011 => 42,
		0b0100 => 44,
		0b0101 => 48,
		0b0110 => 52,
		_ => panic!("Invalid physical address range"),
	};
	info!("Physical address range: {}GB", 1 << (pa_range - 30));

	let tgran16: u64 = (aa64mmfr0 >> 20) & 0b1111;
	let tgran64: u64 = (aa64mmfr0 >> 24) & 0b1111;
	let tgran4: u64 = (aa64mmfr0 >> 28) & 0b1111;

	info!("Support of 4KB pages: {}", tgran4 == 0);
	info!("Support of 16KB pages: {}", tgran16 == 0b0001);
	info!("Support of 64KB pages: {}", tgran64 == 0);

	assert!(tgran4 == 0);

	// page tables are already initialized, we have just to remove obsolete entries
}
