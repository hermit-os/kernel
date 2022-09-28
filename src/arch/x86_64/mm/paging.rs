use core::marker::PhantomData;
use core::mem;
use core::ptr;
use multiboot::information::Multiboot;
use x86::controlregs;
use x86::irq::PageFaultError;
use x86_64::structures::paging::{
	Mapper, PhysFrame, RecursivePageTable, Size1GiB, Size2MiB, Size4KiB,
};

#[cfg(feature = "smp")]
use crate::arch::x86_64::kernel::apic;
use crate::arch::x86_64::kernel::get_mbinfo;
use crate::arch::x86_64::kernel::irq;
use crate::arch::x86_64::kernel::processor;
use crate::arch::x86_64::mm::physicalmem;
use crate::arch::x86_64::mm::{PhysAddr, VirtAddr, MEM};
use crate::env;
use crate::mm;
use crate::scheduler;

/// Pointer to the root page table (PML4)
const PML4_ADDRESS: VirtAddr = VirtAddr(0xFFFF_FFFF_FFFF_F000);

/// Number of Offset bits of a virtual address for a 4 KiB page, which are shifted away to get its Page Frame Number (PFN).
const PAGE_BITS: usize = 12;

/// Number of bits of the index in each table (PML4, PDPT, PD, PT).
const PAGE_MAP_BITS: usize = 9;

/// A mask where PAGE_MAP_BITS are set to calculate a table index.
const PAGE_MAP_MASK: usize = 0x1FF;

pub use x86_64::structures::paging::PageTableFlags as PageTableEntryFlags;

pub trait PageTableEntryFlagsExt {
	fn device(&mut self) -> &mut Self;

	fn normal(&mut self) -> &mut Self;

	fn read_only(&mut self) -> &mut Self;

	fn writable(&mut self) -> &mut Self;

	fn execute_disable(&mut self) -> &mut Self;
}

impl PageTableEntryFlagsExt for PageTableEntryFlags {
	fn device(&mut self) -> &mut Self {
		self.insert(PageTableEntryFlags::NO_CACHE);
		self
	}

	fn normal(&mut self) -> &mut Self {
		self.remove(PageTableEntryFlags::NO_CACHE);
		self
	}

	fn read_only(&mut self) -> &mut Self {
		self.remove(PageTableEntryFlags::WRITABLE);
		self
	}

	fn writable(&mut self) -> &mut Self {
		self.insert(PageTableEntryFlags::WRITABLE);
		self
	}

	fn execute_disable(&mut self) -> &mut Self {
		self.insert(PageTableEntryFlags::NO_EXECUTE);
		self
	}
}

/// An entry in either table (PML4, PDPT, PD, PT)
#[derive(Clone, Copy)]
pub struct PageTableEntry {
	/// Physical memory address this entry refers, combined with flags from PageTableEntryFlags.
	physical_address_and_flags: PhysAddr,
}

impl PageTableEntry {
	/// Return the stored physical address.
	pub fn address(self) -> PhysAddr {
		PhysAddr(
			self.physical_address_and_flags.as_u64()
				& !(BasePageSize::SIZE - 1u64)
				& !(PageTableEntryFlags::NO_EXECUTE).bits(),
		)
	}

	/// Returns whether this entry is valid (present).
	fn is_present(self) -> bool {
		(self.physical_address_and_flags & PageTableEntryFlags::PRESENT.bits()) != 0
	}
}

/// A generic interface to support all possible page sizes.
///
/// This is defined as a subtrait of Copy to enable #[derive(Clone, Copy)] for Page.
/// Currently, deriving implementations for these traits only works if all dependent types implement it as well.
pub trait PageSize: Copy {
	/// The page size in bytes.
	const SIZE: u64;

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
	const SIZE: u64 = 4096;
	const MAP_LEVEL: usize = 0;
	const MAP_EXTRA_FLAG: PageTableEntryFlags = PageTableEntryFlags::empty();
}

/// A 2 MiB page mapped in the PD.
#[derive(Clone, Copy)]
pub enum LargePageSize {}
impl PageSize for LargePageSize {
	const SIZE: u64 = 2 * 1024 * 1024;
	const MAP_LEVEL: usize = 1;
	const MAP_EXTRA_FLAG: PageTableEntryFlags = PageTableEntryFlags::HUGE_PAGE;
}

/// A 1 GiB page mapped in the PDPT.
#[derive(Clone, Copy)]
pub enum HugePageSize {}
impl PageSize for HugePageSize {
	const SIZE: u64 = 1024 * 1024 * 1024;
	const MAP_LEVEL: usize = 2;
	const MAP_EXTRA_FLAG: PageTableEntryFlags = PageTableEntryFlags::HUGE_PAGE;
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
	fn address(self) -> VirtAddr {
		self.virtual_address
	}

	/// Returns whether the given virtual address is a valid one in the x86-64 memory model.
	///
	/// Most x86-64 supports only 48-bit for virtual memory addresses.
	/// Currently, we supports only the lower half of the canonical address space.
	/// As a consequence, the address space is divided into the two valid regions 0x8000_0000_0000
	/// and 0x0000_8000_0000_0000.
	///
	/// Although we could make this check depend on the actual linear address width from the CPU,
	/// any extension above 48-bit would require a new page table level, which we don't implement.
	fn is_valid_address(virtual_address: VirtAddr) -> bool {
		virtual_address < VirtAddr(0x0000_8000_0000_0000u64)
			|| virtual_address >= VirtAddr(0x0000_8000_0000_0000u64)
	}

	/// Returns a Page including the given virtual address.
	/// That means, the address is rounded down to a page size boundary.
	fn including_address(virtual_address: VirtAddr) -> Self {
		assert!(
			Self::is_valid_address(virtual_address),
			"Virtual address {:#X} is invalid",
			virtual_address
		);

		if S::SIZE == 1024 * 1024 * 1024 {
			assert!(processor::supports_1gib_pages());
		}

		Self {
			virtual_address: align_down!(virtual_address, S::SIZE as usize),
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
#[allow(clippy::upper_case_acronyms)]
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
	fn get_page_table_entry<S: PageSize>(&mut self, page: Page<S>) -> Option<PageTableEntry>;
}

impl<L: PageTableLevel> PageTableMethods for PageTable<L> {
	/// Returns the PageTableEntry for the given page if it is present, otherwise returns None.
	///
	/// This is the default implementation called only for PT.
	/// It is overridden by a specialized implementation for all tables with sub tables (all except PT).
	default fn get_page_table_entry<S: PageSize>(
		&mut self,
		page: Page<S>,
	) -> Option<PageTableEntry> {
		assert_eq!(L::LEVEL, S::MAP_LEVEL);
		let index = page.table_index::<L>();

		if self.entries[index].is_present() {
			Some(self.entries[index])
		} else {
			None
		}
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
	fn get_page_table_entry<S: PageSize>(&mut self, page: Page<S>) -> Option<PageTableEntry> {
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
}

impl<L: PageTableLevelWithSubtables> PageTable<L>
where
	L::SubtableLevel: PageTableLevel,
{
	/// Returns the next subtable for the given page in the page table hierarchy.
	///
	/// Must only be called if a page of this size is mapped in a subtable!
	fn subtable<S: PageSize>(&mut self, page: Page<S>) -> &mut PageTable<L::SubtableLevel> {
		assert!(L::LEVEL > S::MAP_LEVEL);

		// Calculate the address of the subtable.
		let index = page.table_index::<L>();
		let table_address = self as *mut PageTable<L> as usize;
		let subtable_address = (table_address << PAGE_MAP_BITS) | (index << PAGE_BITS);
		unsafe { &mut *(subtable_address as *mut PageTable<L::SubtableLevel>) }
	}
}

pub extern "x86-interrupt" fn page_fault_handler(
	stack_frame: irq::ExceptionStackFrame,
	error_code: u64,
) {
	let virtual_address = unsafe { controlregs::cr2() };

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
fn get_page_range<S: PageSize>(virtual_address: VirtAddr, count: usize) -> PageIter<S> {
	let first_page = Page::<S>::including_address(virtual_address);
	let last_page = Page::<S>::including_address(virtual_address + (count as u64 - 1) * S::SIZE);
	Page::range(first_page, last_page)
}

pub fn get_page_table_entry<S: PageSize>(virtual_address: VirtAddr) -> Option<PageTableEntry> {
	trace!("Looking up Page Table Entry for {:#X}", virtual_address);

	let page = Page::<S>::including_address(virtual_address);
	let root_pagetable = unsafe { &mut *(PML4_ADDRESS.as_mut_ptr() as *mut PageTable<PML4>) };
	root_pagetable.get_page_table_entry(page)
}

/// Translate a virtual memory address to a physical one.
pub fn virtual_to_physical(virtual_address: VirtAddr) -> PhysAddr {
	let mut page_bits: u64 = 39;

	// A self-reference enables direct access to all page tables
	static SELF: [VirtAddr; 4] = {
		[
			VirtAddr(0xFFFFFF8000000000u64),
			VirtAddr(0xFFFFFFFFC0000000u64),
			VirtAddr(0xFFFFFFFFFFE00000u64),
			VirtAddr(0xFFFFFFFFFFFFF000u64),
		]
	};

	for i in (0..3).rev() {
		page_bits -= PAGE_MAP_BITS as u64;

		let vpn = (virtual_address.as_u64() >> page_bits) as isize;
		let ptr = SELF[i].as_ptr::<u64>();
		let entry = unsafe { *ptr.offset(vpn) };

		if entry & PageTableEntryFlags::HUGE_PAGE.bits() != 0 || i == 0 {
			let off = virtual_address.as_u64()
				& !(((!0u64) << page_bits) & !PageTableEntryFlags::NO_EXECUTE.bits());
			let phys = entry & (((!0u64) << page_bits) & !PageTableEntryFlags::NO_EXECUTE.bits());

			return PhysAddr(off | phys);
		}
	}

	panic!("virtual_to_physical should never reach this point");
}

#[no_mangle]
pub extern "C" fn virt_to_phys(virtual_address: VirtAddr) -> PhysAddr {
	virtual_to_physical(virtual_address)
}

/// Maps a continuous range of pages.
///
/// # Arguments
///
/// * `physical_address` - First physical address to map these pages to
/// * `flags` - Flags from PageTableEntryFlags to set for the page table entry (e.g. WRITABLE or NO_EXECUTE).
///             The PRESENT, ACCESSED, and DIRTY flags are already set automatically.
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

	let mut current_physical_address = physical_address;
	let mut send_ipi = false;

	for page in range {
		send_ipi |= map_page::<S>(page, current_physical_address, flags);
		current_physical_address += S::SIZE;
	}

	if send_ipi {
		#[cfg(feature = "smp")]
		apic::ipi_tlb_flush();
	}
}

unsafe fn level_4_table() -> &'static mut x86_64::structures::paging::PageTable {
	unsafe { &mut *(PML4_ADDRESS.as_mut_ptr() as *mut x86_64::structures::paging::PageTable) }
}

unsafe fn recursive_page_table() -> RecursivePageTable<'static> {
	unsafe { RecursivePageTable::new(level_4_table()).unwrap() }
}

fn map_page<S: PageSize>(page: Page<S>, phys_addr: PhysAddr, flags: PageTableEntryFlags) -> bool {
	use x86_64::{
		structures::paging::{Page, PageSize},
		PhysAddr, VirtAddr,
	};

	trace!(
		"Mapping {} to {phys_addr:p} ({}) with {flags:?}",
		page.address(),
		S::SIZE
	);

	let flags = flags
		| PageTableEntryFlags::PRESENT
		| PageTableEntryFlags::ACCESSED
		| PageTableEntryFlags::DIRTY;

	match S::SIZE {
		Size4KiB::SIZE => {
			let page =
				Page::<Size4KiB>::from_start_address(VirtAddr::new(page.address().0)).unwrap();
			let frame = PhysFrame::from_start_address(PhysAddr::new(phys_addr.0)).unwrap();
			unsafe {
				// TODO: Require explicit unmaps
				if let Ok((_frame, flush)) = recursive_page_table().unmap(page) {
					flush.flush();
				}
				recursive_page_table()
					.map_to(page, frame, flags, &mut physicalmem::FrameAlloc)
					.unwrap()
					.flush();
			}
		}
		Size2MiB::SIZE => {
			let page =
				Page::<Size2MiB>::from_start_address(VirtAddr::new(page.address().0)).unwrap();
			let frame = PhysFrame::from_start_address(PhysAddr::new(phys_addr.0)).unwrap();
			unsafe {
				recursive_page_table()
					.map_to(page, frame, flags, &mut physicalmem::FrameAlloc)
					.unwrap()
					.flush();
			}
		}
		Size1GiB::SIZE => {
			let page =
				Page::<Size1GiB>::from_start_address(VirtAddr::new(page.address().0)).unwrap();
			let frame = PhysFrame::from_start_address(PhysAddr::new(phys_addr.0)).unwrap();
			unsafe {
				recursive_page_table()
					.map_to(page, frame, flags, &mut physicalmem::FrameAlloc)
					.unwrap()
					.flush();
			}
		}
		_ => unreachable!(),
	}

	true
}

pub fn unmap<S: PageSize>(virtual_address: VirtAddr, count: usize) {
	trace!(
		"Unmapping virtual address {:#X} ({} pages)",
		virtual_address,
		count
	);

	map::<S>(
		virtual_address,
		PhysAddr::zero(),
		count,
		PageTableEntryFlags::empty(),
	);
}

pub fn identity_map(start_address: PhysAddr, end_address: PhysAddr) {
	let first_page = Page::<BasePageSize>::including_address(VirtAddr(start_address.as_u64()));
	let last_page = Page::<BasePageSize>::including_address(VirtAddr(end_address.as_u64()));
	assert!(
		last_page.address() < mm::kernel_start_address(),
		"Address {:#X} to be identity-mapped is not below Kernel start address",
		last_page.address()
	);

	let mut flags = PageTableEntryFlags::empty();
	flags.normal().read_only().execute_disable();
	let count = (last_page.address().0 - first_page.address().0) / BasePageSize::SIZE + 1;
	map::<BasePageSize>(
		first_page.address(),
		PhysAddr(first_page.address().0),
		count as usize,
		flags,
	);
}

#[inline]
pub fn get_application_page_size() -> usize {
	LargePageSize::SIZE as usize
}

pub fn init() {}

pub fn init_page_tables() {
	debug!("Create new view to the kernel space");

	unsafe {
		let pml4 = controlregs::cr3();
		let pde = pml4 + 2 * BasePageSize::SIZE;

		debug!("Found PML4 at {:#x}", pml4);

		// make sure that only the required areas are mapped
		let start = pde
			+ ((mm::kernel_end_address().as_usize() >> (PAGE_MAP_BITS + PAGE_BITS))
				* mem::size_of::<u64>()) as u64;
		let size = (512 - (mm::kernel_end_address().as_usize() >> (PAGE_MAP_BITS + PAGE_BITS)))
			* mem::size_of::<u64>();

		ptr::write_bytes(start as *mut u8, 0u8, size);

		//TODO: clearing the memory before kernel_start_address()

		// flush tlb
		controlregs::cr3_write(pml4);

		// Identity-map the supplied Multiboot information and command line.
		let mb_info = get_mbinfo();
		if !mb_info.is_zero() {
			info!("Found Multiboot info at {:#x}", mb_info);
			identity_map(PhysAddr(mb_info.as_u64()), PhysAddr(mb_info.as_u64()));

			// Map the "Memory Map" information too.
			let mb = Multiboot::from_ptr(mb_info.as_u64(), &mut MEM).unwrap();
			let memory_map_address = mb
				.memory_regions()
				.expect("Could not find a memory map in the Multiboot information")
				.next()
				.expect("Could not first map address")
				.base_address();
			identity_map(PhysAddr(memory_map_address), PhysAddr(memory_map_address));
		}

		let cmdsize = env::get_cmdsize();
		if cmdsize > 0 {
			let cmdline = env::get_cmdline();
			info!("Found cmdline at {:#x} (size {})", cmdline, cmdsize);
			identity_map(
				PhysAddr(cmdline.as_u64()),
				PhysAddr(cmdline.as_u64()) + cmdsize - 1u64,
			);
		}
	}
}
