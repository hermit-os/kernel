use core::marker::PhantomData;
use core::usize;

use align_address::Align;
use riscv::asm::sfence_vma;
use riscv::register::satp;

use crate::arch::riscv64::kernel::get_ram_address;
use crate::arch::riscv64::mm::{physicalmem, PhysAddr, VirtAddr};

static mut ROOT_PAGETABLE: PageTable<L2Table> = PageTable::new();

/// Number of Offset bits of a virtual address for a 4 KiB page, which are shifted away to get its Page Frame Number (PFN).
const PAGE_BITS: usize = 12;

/// Number of bits of the index in each table
const PAGE_MAP_BITS: usize = 9;

/// A mask where PAGE_MAP_BITS are set to calculate a table index.
const PAGE_MAP_MASK: usize = 0x1FF;

/// Number of page levels
const PAGE_LEVELS: usize = 3;

bitflags! {
	/// Flags for an PTE
	///
	/// See The RISC-V Instruction Set Manual Volume II: Privileged Architecture
	#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
	pub struct PageTableEntryFlags: u64 {
		/// Set if this entry is valid.
		const VALID = 1 << 0;

		/// Set if this page is readable
		const READABLE = 1 << 1;

		/// Set if this page is writable
		const WRITABLE = 1 << 2;

		/// Set if this page is executable
		const EXECUTABLE = 1 << 3;

		/// Set if memory referenced by this entry shall be accessible from user-mode
		const USER_ACCESSIBLE = 1 << 4;

		/// Set if mapping exists in all address spaces
		const GLOBAL = 1 << 5;

		/// Set if software has accessed this entry
		const ACCESSED = 1 << 6;

		/// Only for page entries: Set if software has written to the memory referenced by this entry.
		const DIRTY = 1 << 7;

		/// The RSW field is reserved for use by supervisor
		const RSW  = 1 << 8 | 1 << 9;
	}
}

#[allow(dead_code)]
impl PageTableEntryFlags {
	/// An empty set of flags for unused/zeroed table entries.
	/// Needed as long as empty() is no const function.
	const BLANK: PageTableEntryFlags = PageTableEntryFlags::empty();

	pub fn device(&mut self) -> &mut Self {
		self
	}

	pub fn normal(&mut self) -> &mut Self {
		self.insert(PageTableEntryFlags::EXECUTABLE);
		self.insert(PageTableEntryFlags::READABLE);
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
		self.remove(PageTableEntryFlags::EXECUTABLE);
		self
	}
}

/// An entry in either table
#[derive(Clone, Copy, Debug)]
pub struct PageTableEntry {
	/// Physical memory address this entry refers, combined with flags from PageTableEntryFlags.
	physical_address_and_flags: PhysAddr,
}

#[allow(dead_code)]
impl PageTableEntry {
	/// Return the stored physical address.
	pub fn address(&self) -> PhysAddr {
		PhysAddr(
			(
				self.physical_address_and_flags.as_u64() & !(0x3FFu64)
				//& !(0x3FFu64 << 54)
			) << 2,
		)
	}

	/// Returns whether this entry is valid (present).
	fn is_present(&self) -> bool {
		(self.physical_address_and_flags & PageTableEntryFlags::VALID.bits()) != 0
	}

	/// Returns `true` if the page is accessible from the user space
	fn is_user(self) -> bool {
		(self.physical_address_and_flags & PageTableEntryFlags::USER_ACCESSIBLE.bits()) != 0
	}

	/// Returns `true` if the page is readable
	fn is_readable(self) -> bool {
		(self.physical_address_and_flags & PageTableEntryFlags::READABLE.bits()) != 0
	}

	/// Returns `true` if the page is writable
	fn is_writable(self) -> bool {
		(self.physical_address_and_flags & PageTableEntryFlags::WRITABLE.bits()) != 0
	}

	/// Returns `true` if the page is executable
	fn is_executable(self) -> bool {
		(self.physical_address_and_flags & PageTableEntryFlags::EXECUTABLE.bits()) != 0
	}

	/// Mark this as a valid (present) entry and set address translation and flags.
	///
	/// # Arguments
	///
	/// * `physical_address` - The physical memory address this entry shall translate to
	/// * `flags` - Flags from PageTableEntryFlags (note that the VALID, GLOBAL, DIRTY and ACCESSED flags are set)
	fn set(&mut self, physical_address: PhysAddr, flags: PageTableEntryFlags) {
		// Verify that the offset bits for a 4 KiB page are zero.
		assert_eq!(
			physical_address % BasePageSize::SIZE as usize,
			0,
			"Physical address is not on a 4 KiB page boundary (physical_address = {physical_address:#X})"
		);

		let mut flags_to_set = flags;
		flags_to_set.insert(PageTableEntryFlags::VALID);
		flags_to_set.insert(PageTableEntryFlags::GLOBAL);
		self.physical_address_and_flags =
			PhysAddr((physical_address.as_u64() >> 2) | flags_to_set.bits());
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
	const MAP_LEVEL: usize = 0;
	const MAP_EXTRA_FLAG: PageTableEntryFlags = PageTableEntryFlags::BLANK;
}

/// A 2 MiB page mapped in the L2Table.
#[derive(Clone, Copy)]
pub enum LargePageSize {}
impl PageSize for LargePageSize {
	const SIZE: u64 = 2 * 1024 * 1024;
	const MAP_LEVEL: usize = 1;
	const MAP_EXTRA_FLAG: PageTableEntryFlags = PageTableEntryFlags::BLANK;
}

/// A 1 GiB page mapped in the L1Table.
#[derive(Clone, Copy)]
pub enum HugePageSize {}
impl PageSize for HugePageSize {
	const SIZE: u64 = 1024 * 1024 * 1024;
	const MAP_LEVEL: usize = 2;
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
		unsafe {
			sfence_vma(0, self.virtual_address.as_usize());
		}
	}

	/// Returns whether the given virtual address is a valid one in SV39
	/// The address is valid when bit 38 and all more significant bits match
	fn is_valid_address(virtual_address: VirtAddr) -> bool {
		if virtual_address.as_u64() & (1 << 38) != 0 {
			virtual_address.as_u64() >> 39 == (1 << (64 - 39)) - 1
		} else {
			virtual_address.as_u64() >> 39 == 0
		}
	}

	/// Returns a Page including the given virtual address.
	/// That means, the address is rounded down to a page size boundary.
	fn including_address(virtual_address: VirtAddr) -> Self {
		assert!(
			Self::is_valid_address(virtual_address),
			"Virtual address {virtual_address:#X} is invalid"
		);

		Self {
			virtual_address: VirtAddr(virtual_address.0.align_down(S::SIZE)),
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
	fn table_index<L: PageTableLevel>(self) -> usize {
		assert!(L::LEVEL >= S::MAP_LEVEL);
		self.virtual_address.as_usize() >> PAGE_BITS >> (L::LEVEL * PAGE_MAP_BITS) & PAGE_MAP_MASK
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

/// The Level 2 Table (can map 1 GiB pages)
enum L2Table {}
impl PageTableLevel for L2Table {
	const LEVEL: usize = 2;
}

impl PageTableLevelWithSubtables for L2Table {
	type SubtableLevel = L1Table;
}

/// The Level 1 Table (can map 2 MiB pages)
enum L1Table {}
impl PageTableLevel for L1Table {
	const LEVEL: usize = 1;
}

impl PageTableLevelWithSubtables for L1Table {
	type SubtableLevel = L0Table;
}

/// The Level 0 Table (can map 4 KiB pages)
enum L0Table {}
impl PageTableLevel for L0Table {
	const LEVEL: usize = 0;
}

/// Representation of any page table in memory.
/// Parameter L supplies information for Rust's typing system to distinguish between the different tables.
#[repr(align(4096))]
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

impl<L: PageTableLevel> PageTable<L> {
	const fn new() -> Self {
		PageTable {
			entries: [PageTableEntry {
				physical_address_and_flags: PhysAddr::zero(),
			}; 1 << PAGE_MAP_BITS],
			level: PhantomData,
		}
	}
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

		self.entries[index].set(
			physical_address,
			S::MAP_EXTRA_FLAG | PageTableEntryFlags::ACCESSED | PageTableEntryFlags::DIRTY | flags,
		);

		if flush {
			page.flush_from_tlb();
		}
	}

	/// Returns the PageTableEntry for the given page if it is present, otherwise returns None.
	///
	/// This is the default implementation called only for L0Table.
	/// It is overridden by a specialized implementation for all tables with sub tables (all except L0Table).
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
		self.map_page_in_this_table::<S>(page, physical_address, flags)
	}
}

impl<L: PageTableLevelWithSubtables> PageTableMethods for PageTable<L>
where
	L::SubtableLevel: PageTableLevel,
{
	/// Returns the PageTableEntry for the given page if it is present, otherwise returns None.
	///
	/// This is the implementation for all tables with subtables (L1, L2).
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
	///
	/// This is the implementation for all tables with subtables (L1, L2).
	/// It overrides the default implementation above.
	fn map_page<S: PageSize>(
		&mut self,
		page: Page<S>,
		physical_address: PhysAddr,
		flags: PageTableEntryFlags,
	) {
		assert!(L::LEVEL >= S::MAP_LEVEL);

		// trace!(
		// 	"Mapping frame {:#X} to page {:#X}",
		// 	physical_address,
		// 	page.virtual_address,
		// );

		if L::LEVEL > S::MAP_LEVEL {
			let index = page.table_index::<L>();

			// trace!("L::LEVEL > S::MAP_LEVEL");

			// trace!("self.entries[index] {:?} , index {}",self.entries[index], index);

			// Does the table exist yet?
			if !self.entries[index].is_present() {
				// Allocate a single 4 KiB page for the new entry and mark it as a valid, writable subtable.
				let new_entry = physicalmem::allocate(BasePageSize::SIZE as usize).unwrap();
				self.entries[index].set(new_entry, PageTableEntryFlags::BLANK);

				// trace!("new_entry {:#X}", new_entry);

				// Mark all entries as unused in the newly created table.
				let subtable = self.subtable::<S>(page);
				for entry in subtable.entries.iter_mut() {
					entry.physical_address_and_flags = PhysAddr::zero();
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
	// FIXME: https://github.com/hermit-os/kernel/issues/877
	#[allow(clippy::mut_from_ref)]
	fn subtable<S: PageSize>(&self, page: Page<S>) -> &mut PageTable<L::SubtableLevel> {
		assert!(L::LEVEL > S::MAP_LEVEL);

		// Calculate the address of the subtable.
		let index = page.table_index::<L>();
		// trace!("Index: {:#X}", index);
		let subtable_address = self.entries[index].address().as_usize();
		// trace!("subtable_address: {:#X}", subtable_address);
		unsafe { &mut *(subtable_address as *mut PageTable<L::SubtableLevel>) }
	}

	/// Maps a continuous range of pages.
	///
	/// # Arguments
	///
	/// * `range` - The range of pages of size S
	/// * `physical_address` - First physical address to map these pages to
	/// * `flags` - Flags from PageTableEntryFlags to set for the page table entry (e.g. WRITABLE or NO_EXECUTE).
	///             The VALID and GLOBAL are already set automatically.
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
	let last_page = Page::<S>::including_address(virtual_address + (count - 1) * S::SIZE as usize);
	Page::range(first_page, last_page)
}

/// Translate a virtual memory address to a physical one.
/// Just like get_physical_address, but automatically uses the correct page size for the respective memory address.
pub fn virtual_to_physical(virtual_address: VirtAddr) -> Option<PhysAddr> {
	// panic!("Not impemented!");
	/* if virtual_address < mm::kernel_start_address() {
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
	} */
	let mut vpn: [u64; PAGE_LEVELS] = [0; PAGE_LEVELS];

	for (i, item) in vpn.iter_mut().enumerate().take(PAGE_LEVELS) {
		*item = (virtual_address >> (PAGE_BITS + i * PAGE_MAP_BITS)) & PAGE_MAP_MASK as u64;
		trace!(
			"i: {}, vpn[i]: {:#X}, {:#X}",
			i,
			*item,
			virtual_address >> (PAGE_BITS + i * PAGE_MAP_BITS)
		);
	}

	let mut page_table_addr = unsafe { &ROOT_PAGETABLE as *const PageTable<L2Table> };
	for i in (0..PAGE_LEVELS).rev() {
		let pte = unsafe { (*page_table_addr).entries[(vpn[i]) as usize] };
		// trace!("PTE: {:?} , i: {}, vpn[i]: {:#X}", pte, i, vpn[i]);
		//Translation would raise a page-fault exception
		assert!(
			pte.is_present() && (pte.is_readable() || !pte.is_writable()),
			"Invalid PTE: {pte:?}"
		);

		if pte.is_executable() || pte.is_readable() {
			//PTE is a leaf
			// trace!("PTE is a leaf");
			let mut phys_address = virtual_address.as_u64() & ((1 << PAGE_BITS) - 1);
			for (j, item) in vpn.iter().enumerate().take(i) {
				phys_address |= (item) << (PAGE_BITS + j * PAGE_MAP_BITS);
			}
			let ppn = pte.address().as_u64();
			for j in i..PAGE_LEVELS {
				// trace!(
				// 	"ppn: {:#X}, {:#X}",
				// 	ppn,
				// 	ppn & (PAGE_MAP_MASK << (PAGE_BITS + j * PAGE_MAP_BITS)) as u64
				// );
				phys_address |= ppn & (PAGE_MAP_MASK << (PAGE_BITS + j * PAGE_MAP_BITS)) as u64;
			}
			return Some(PhysAddr(phys_address));
		} else {
			//PTE is a pointer to the next level of the page table
			assert!(i != 0); //pte should be a leaf if i=0
			page_table_addr = pte.address().as_usize() as *mut PageTable<L2Table>;
			// trace!("PTE is pointer: {:?}", page_table_addr);
		}
	}
	panic!("virtual_to_physical should never reach this point");
}

#[no_mangle]
pub extern "C" fn virt_to_phys(virtual_address: VirtAddr) -> PhysAddr {
	virtual_to_physical(virtual_address).unwrap()
}

pub fn map<S: PageSize>(
	virtual_address: VirtAddr,
	physical_address: PhysAddr,
	count: usize,
	flags: PageTableEntryFlags,
) {
	trace!(
		"Mapping physical address {:#X} to virtual address {:#X} ({} pages)",
		physical_address,
		virtual_address,
		count
	);

	let range = get_page_range::<S>(virtual_address, count);
	unsafe {
		ROOT_PAGETABLE.map_pages(range, physical_address, flags);
	}

	//assert_eq!(virtual_address.as_u64(), physical_address.as_u64(), "Paging not implemented");
}

pub fn map_heap<S: PageSize>(virt_addr: VirtAddr, count: usize) -> Result<(), usize> {
	let flags = {
		let mut flags = PageTableEntryFlags::empty();
		flags.normal().writable().execute_disable();
		flags
	};

	let virt_addrs = (0..count).map(|n| virt_addr + n * S::SIZE as usize);

	for (map_counter, virt_addr) in virt_addrs.enumerate() {
		let phys_addr = physicalmem::allocate_aligned(S::SIZE as usize, S::SIZE as usize)
			.map_err(|_| map_counter)?;
		map::<S>(virt_addr, phys_addr, 1, flags);
	}

	Ok(())
}

pub fn unmap<S: PageSize>(virtual_address: VirtAddr, count: usize) {
	trace!(
		"Unmapping virtual address {:#X} ({} pages)",
		virtual_address,
		count
	);

	let range = get_page_range::<S>(virtual_address, count);
	/* let root_pagetable = unsafe {
		&mut *mem::transmute::<*mut u64, *mut PageTable<L2Table>>(L2TABLE_ADDRESS.as_mut_ptr())
	}; */
	unsafe {
		ROOT_PAGETABLE.map_pages(range, PhysAddr::zero(), PageTableEntryFlags::BLANK);
	}
}

#[inline]
pub fn get_application_page_size() -> usize {
	LargePageSize::SIZE as usize
}

pub fn identity_map<S: PageSize>(start_address: PhysAddr, end_address: PhysAddr) {
	let first_page = Page::<S>::including_address(VirtAddr(start_address.as_u64()));
	let last_page = Page::<S>::including_address(VirtAddr(end_address.as_u64()));

	trace!(
		"identity_map address {:#X} to address {:#X}",
		first_page.virtual_address,
		last_page.virtual_address,
	);

	/* assert!(
		last_page.address() < mm::kernel_start_address(),
		"Address {:#X} to be identity-mapped is not below Kernel start address",
		last_page.address()
	); */

	/* let root_pagetable = unsafe {
		&mut *mem::transmute::<*mut u64, *mut PageTable<L2Table>>(L2TABLE_ADDRESS.as_mut_ptr())
	}; */
	let range = Page::<S>::range(first_page, last_page);
	let mut flags = PageTableEntryFlags::empty();
	flags.normal().writable();
	unsafe {
		ROOT_PAGETABLE.map_pages(range, PhysAddr(first_page.address().as_u64()), flags);
	}
}

pub fn init_page_tables() {
	trace!("Identity map the physical memory using HugePages");

	unsafe {
		identity_map::<HugePageSize>(
			get_ram_address(),
			get_ram_address() + PhysAddr(physicalmem::total_memory_size() as u64 - 1),
		);
		satp::write(0x8 << 60 | ((&ROOT_PAGETABLE as *const _ as usize) >> 12))
	}
}

#[cfg(feature = "smp")]
pub fn init_application_processor() {
	trace!("Identity map the physical memory using HugePages");
	unsafe { satp::write(0x8 << 60 | ((&ROOT_PAGETABLE as *const _ as usize) >> 12)) }
}
