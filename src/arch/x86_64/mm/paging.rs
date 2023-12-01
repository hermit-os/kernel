use core::fmt::Debug;
use core::ptr;

use env::kernel::pit::PIT_INTERRUPT_NUMBER;
use x86_64::instructions::tlb;
use x86_64::registers::control::{Cr0, Cr0Flags, Cr3};
use x86_64::structures::paging::frame::PhysFrameRange;
use x86_64::structures::paging::mapper::{
	MapToError, MappedFrame, MapperAllSizes, TranslateResult, UnmapError,
};
use x86_64::structures::paging::page_table::PageTableLevel;
pub use x86_64::structures::paging::PageTableFlags as PageTableEntryFlags;
use x86_64::structures::paging::{
	self, FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PageTableIndex, PhysFrame,
	RecursivePageTable, Translate,
};

use crate::arch::x86_64::mm::{physicalmem, PhysAddr, VirtAddr};
use crate::{env, mm};

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

pub use x86_64::structures::paging::{
	PageSize, Size1GiB as HugePageSize, Size2MiB as LargePageSize, Size4KiB as BasePageSize,
};

unsafe fn recursive_page_table() -> RecursivePageTable<'static> {
	let level_4_table_addr = 0xFFFF_FFFF_FFFF_F000;
	let level_4_table_ptr = ptr::from_exposed_addr_mut(level_4_table_addr);
	unsafe {
		let level_4_table = &mut *(level_4_table_ptr);
		RecursivePageTable::new(level_4_table).unwrap()
	}
}

unsafe fn identity_mapped_page_table() -> OffsetPageTable<'static> {
	let level_4_table_addr = Cr3::read().0.start_address().as_u64();
	let level_4_table_ptr =
		ptr::from_exposed_addr_mut::<PageTable>(level_4_table_addr.try_into().unwrap());
	unsafe {
		let level_4_table = &mut *(level_4_table_ptr);
		OffsetPageTable::new(level_4_table, x86_64::addr::VirtAddr::new(0x0))
	}
}

/// Translate a virtual memory address to a physical one.
pub fn virtual_to_physical(virtual_address: VirtAddr) -> Option<PhysAddr> {
	let virtual_address = x86_64::VirtAddr::new(virtual_address.0);
	let page_table = unsafe { recursive_page_table() };
	page_table
		.translate_addr(virtual_address)
		.map(|addr| PhysAddr(addr.as_u64()))
}

#[no_mangle]
pub extern "C" fn virt_to_phys(virtual_address: VirtAddr) -> PhysAddr {
	virtual_to_physical(virtual_address).unwrap()
}

/// Maps a continuous range of pages.
///
/// # Arguments
///
/// * `physical_address` - First physical address to map these pages to
/// * `flags` - Flags from PageTableEntryFlags to set for the page table entry (e.g. WRITABLE or NO_EXECUTE).
///             The PRESENT flags is set automatically.
pub fn map<S>(
	virtual_address: VirtAddr,
	physical_address: PhysAddr,
	count: usize,
	flags: PageTableEntryFlags,
) where
	S: PageSize + Debug,
	RecursivePageTable<'static>: Mapper<S> + MapperAllSizes,
	OffsetPageTable<'static>: Mapper<S> + MapperAllSizes,
{
	let pages = {
		let start = Page::<S>::containing_address(x86_64::VirtAddr::new(virtual_address.0));
		let end = start + count as u64;
		Page::range(start, end)
	};

	let frames = {
		let start = PhysFrame::<S>::containing_address(x86_64::PhysAddr::new(physical_address.0));
		let end = start + count as u64;
		PhysFrame::range(start, end)
	};

	let flags = flags | PageTableEntryFlags::PRESENT;

	trace!("Mapping {pages:?} to {frames:?} with {flags:?}");

	#[cfg(feature = "smp")]
	let mut ipi_tlb_flush = false;

	if crate::arch::x86_64::kernel::is_uefi().is_err() {
		for (page, frame) in pages.zip(frames) {
			unsafe {
				trace!("mapping pages to frames");
				// TODO: Require explicit unmaps
				if let Ok((_frame, flush)) = recursive_page_table().unmap(page) {
					#[cfg(feature = "smp")]
					{
						ipi_tlb_flush = true;
					}
					flush.flush();
					debug!("Had to unmap page {page:?} before mapping.");
				}
				recursive_page_table()
					.map_to(page, frame, flags, &mut physicalmem::FrameAlloc)
					.unwrap()
					.flush();
			}
		}
	} else {
		for (page, frame) in pages.zip(frames) {
			unsafe {
				trace!("mapping page {page:#?} to frame {frame:#?}");
				let mut pt = identity_mapped_page_table();
				if let Ok((_frame, flush)) = pt.unmap(page) {
					trace!("unmapped");
					#[cfg(feature = "smp")]
					{
						ipi_tlb_flush = true;
					}
					flush.flush();
					debug!("Had to unmap page {page:?} before mapping.");
				}
				pt.identity_map(frame, flags, &mut physicalmem::FrameAlloc)
					.unwrap_or_else(|e| match e {
						MapToError::ParentEntryHugePage => {
							assert!(
								S::SIZE == BasePageSize::SIZE,
								"Giant Pages are not supported"
							);
							recast_huge_page(pt, page, frame, flags)
						}
						_ => panic!(
							"error {e:?} at: {frame:?} \n pages: 4 {:?}, 3 {:?}, 2 {:?}, 1 {:?} \n",
							page.p4_index(),
							page.p3_index(),
							page.page_table_index(PageTableLevel::Two),
							page.page_table_index(PageTableLevel::One)
						),
					})
					.flush();
			}
		}
	}

	#[cfg(feature = "smp")]
	if ipi_tlb_flush {
		crate::arch::x86_64::kernel::apic::ipi_tlb_flush();
	}
}

/// Maps `count` pages at address `virt_addr`. If the allocation of a physical memory failed,
/// the number of successfull mapped pages are returned as error value.
pub fn map_heap<S: PageSize>(virt_addr: VirtAddr, count: usize) -> Result<(), usize>
where
	S: PageSize + Debug,
	RecursivePageTable<'static>: Mapper<S>,
	OffsetPageTable<'static>: Mapper<S>,
{
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

#[cfg(feature = "acpi")]
pub fn identity_map<S>(frame: PhysFrame<S>)
where
	S: PageSize + Debug,
	RecursivePageTable<'static>: Mapper<S>,
{
	assert!(
		frame.start_address().as_u64() < mm::kernel_start_address().0,
		"Address {:p} to be identity-mapped is not below Kernel start address",
		frame.start_address()
	);

	unsafe {
		recursive_page_table()
			.identity_map(
				frame,
				PageTableEntryFlags::PRESENT | PageTableEntryFlags::NO_EXECUTE,
				&mut physicalmem::FrameAlloc,
			)
			.unwrap()
			.flush();
	}
}

pub fn unmap<S>(virtual_address: VirtAddr, count: usize)
where
	S: PageSize + Debug,
	RecursivePageTable<'static>: Mapper<S>,
{
	trace!(
		"Unmapping virtual address {:p} ({} pages)",
		virtual_address,
		count
	);

	let first_page = Page::<S>::containing_address(x86_64::VirtAddr::new(virtual_address.0));
	let last_page = first_page + count as u64;
	let range = Page::range(first_page, last_page);

	for page in range {
		let mut page_table = unsafe { recursive_page_table() };
		match page_table.unmap(page) {
			Ok((_frame, flush)) => flush.flush(),
			// FIXME: Some sentinel pages around stacks are supposed to be unmapped.
			// We should handle this case there instead of here.
			Err(UnmapError::PageNotMapped) => {
				debug!("Tried to unmap {page:?}, which was not mapped.")
			}
			Err(err) => panic!("{err:?}"),
		}
	}
}

#[inline]
pub fn get_application_page_size() -> usize {
	LargePageSize::SIZE as usize
}

pub fn init() {
	if crate::arch::x86_64::kernel::is_uefi().is_ok() {
		check_root_pagetable();
	}
}

pub fn init_page_tables() {
	if env::is_uhyve() {
		// Uhyve identity-maps the first Gibibyte of memory (512 page table entries * 2MiB pages)
		// We now unmap all memory after the kernel image, so that we can remap it ourselves later for the heap.
		// Ideally, uhyve would only map as much memory as necessary, but this requires a hermit-entry ABI jump.
		// See https://github.com/hermit-os/uhyve/issues/426
		let kernel_end_addr = x86_64::VirtAddr::new(mm::kernel_end_address().as_u64());
		let start_page = Page::<LargePageSize>::from_start_address(kernel_end_addr).unwrap();
		let end_page = Page::from_page_table_indices_2mib(
			start_page.p4_index(),
			start_page.p3_index(),
			PageTableIndex::new(511),
		);
		let page_range = Page::range_inclusive(start_page, end_page);

		let mut page_table = unsafe { recursive_page_table() };
		for page in page_range {
			let (_frame, flush) = page_table.unmap(page).unwrap();
			flush.ignore();
		}

		tlb::flush_all();
	}
}

/// Checks the address stored in the CR3 register and if necessary, makes its page writable.
fn check_root_pagetable() {
	let level_4_table_addr = Cr3::read().0.start_address().as_u64();
	let virt_lvl_4_addr = x86_64::VirtAddr::new(level_4_table_addr);
	let pt = unsafe { identity_mapped_page_table() };
	clear_wp_bit();
	match pt.translate(virt_lvl_4_addr) {
		TranslateResult::Mapped {
			frame,
			offset: _,
			flags,
		} => match frame {
			MappedFrame::Size1GiB(_) => {
				set_pagetable_page_writable(frame, virt_lvl_4_addr, flags, pt);
			}
			MappedFrame::Size2MiB(_) => {
				set_pagetable_page_writable(frame, virt_lvl_4_addr, flags, pt);
			}
			MappedFrame::Size4KiB(_) => {
				set_pagetable_page_writable(frame, virt_lvl_4_addr, flags, pt);
			}
		},
		TranslateResult::NotMapped => todo!(),
		TranslateResult::InvalidFrameAddress(_) => todo!(),
	};
	set_wp_bit();
}

/// Clears the WRITE_PROTECT bit in order to write into read-only Pages in supervisor mode.
fn clear_wp_bit() {
	let mut cr0 = Cr0::read();

	if cr0.contains(Cr0Flags::WRITE_PROTECT) {
		trace!("clear WRITE_PROTECT bit temporarily");
		unsafe {
			cr0.remove(Cr0Flags::WRITE_PROTECT);
			Cr0::write(cr0);
		}
		debug!("Cr0 flags: {:?}", Cr0::read());
	}
}

/// Sets the WRITE_PROTECT bit in order to prevent writing into read-only Pages even in supervisor mode.
fn set_wp_bit() {
	let mut cr0 = Cr0::read();
	unsafe {
		cr0.insert(Cr0Flags::WRITE_PROTECT);
		Cr0::write(cr0);
	}
	debug!("Cr0 flags: {:?}", Cr0::read());
}

/// This function takes the rootpage and depending on its size (1GiB, 2MiB, 4KiB), changes the flags to make it writable and then flushes the TLB.
/// This is useful for memory manipulation.
fn set_pagetable_page_writable(
	framesize: MappedFrame,
	addr: x86_64::VirtAddr,
	flags: PageTableEntryFlags,
	mut pt: OffsetPageTable<'_>,
) {
	let page: Page = Page::from_start_address(addr).unwrap();
	match framesize {
		MappedFrame::Size1GiB(_) => {
			let flush = unsafe {
				pt.set_flags_p3_entry(page, flags | PageTableEntryFlags::WRITABLE)
					.unwrap()
			};
			flush.flush_all();
		}
		MappedFrame::Size2MiB(_) => {
			let flush = unsafe {
				pt.set_flags_p2_entry(page, flags | PageTableEntryFlags::WRITABLE)
					.unwrap()
			};
			flush.flush_all();
		}
		MappedFrame::Size4KiB(_) => {
			let flush = unsafe {
				pt.update_flags(page, flags | PageTableEntryFlags::WRITABLE)
					.unwrap()
			};
			flush.flush();
		}
	}
	trace!("Rootpage now writable");
}

/// This function remaps one given Huge Page (2 MiB) into 512 Normal Pages (4 KiB) with a given (Offset/ID mapped) Page Table.
/// It takes the Page Table with its mapping, the Hugepage in question, the physical frame it is stored in and its flags as input.
/// The page gets remapped and a new level 2 Pagetable is allocated in this function to connect the (now) 512 level 1 pages.
unsafe fn recast_huge_page<S>(
	mut pt: OffsetPageTable<'static>,
	page: Page<S>,
	frame: PhysFrame<S>,
	flags: PageTableEntryFlags,
) -> x86_64::structures::paging::mapper::MapperFlush<S>
where
	S: PageSize + Debug,
	OffsetPageTable<'static>: Mapper<S> + MapperAllSizes,
{
	//make sure that the allocated physical frame is NOT inside the page that needs remapping
	let forbidden_start: PhysFrame<BasePageSize> =
		PhysFrame::from_start_address(frame.start_address()).unwrap();
	let forbidden_end: PhysFrame<BasePageSize> = PhysFrame::from_start_address(
		x86_64::PhysAddr::new(frame.start_address().as_u64() + 0x200000),
	)
	.unwrap();
	trace!("forbidden start: {forbidden_start:?}, forbidden end: {forbidden_end:?}");
	let forbidden_range: PhysFrameRange<BasePageSize> =
		PhysFrame::range(forbidden_start, forbidden_end);
	//Pagetable walk to get the correct data
	let pml4 = pt.level_4_table();
	let pdpte_entry = &mut pml4[page.p4_index()];
	let pdpte_ptr: *mut PageTable = x86_64::VirtAddr::new(pdpte_entry.addr().as_u64()).as_mut_ptr();
	let pdpte = unsafe { &mut *pdpte_ptr };
	let pde_entry = &mut pdpte[page.p3_index()];
	let pde_ptr: *mut PageTable = x86_64::VirtAddr::new(pde_entry.addr().as_u64()).as_mut_ptr();
	let pde = unsafe { &mut *pde_ptr };
	let pte_entry = &mut pde[page.page_table_index(PageTableLevel::Two)];
	let pte_entry_start = pte_entry.addr().as_u64(); //start of HUGE PAGE
												 //allocate new 4KiB frame for pte
												 //let allocator = &mut physicalmem::FrameAlloc;
												 //make sure that this frame is NOT inside the page that is to be recast -> TODO: adapt Allocator
	let pte_frame: PhysFrame<BasePageSize> =
		physicalmem::allocate_outside_of(S::SIZE as usize, S::SIZE as usize, forbidden_range)
			.unwrap();

	trace!("pte_frame: {pte_frame:#?}");
	let new_flags = PageTableEntryFlags::PRESENT | PageTableEntryFlags::WRITABLE;
	let pte_ptr: *mut PageTable = x86_64::VirtAddr::new(pte_entry.addr().as_u64()).as_mut_ptr();
	let pte = unsafe { &mut *pte_ptr };
	//remap everything
	for (i, entry) in pte.iter_mut().enumerate() {
		let addr = pte_entry_start + (i * 0x1000) as u64; //calculates starting addresses of the normal sized pages
		entry.set_addr(x86_64::PhysAddr::new(addr), new_flags)
	}

	pte_entry.set_frame(pte_frame, new_flags);
	trace!("new pte_entry point: {pte_entry:?}");
	trace!("successfully remapped");
	tlb::flush_all(); // flush TLB to ensure all memory is valid and up-to-date
	crate::arch::mm::physicalmem::print_information();
	unsafe {
		pt.identity_map(frame, flags, &mut physicalmem::FrameAlloc)
			.unwrap()
	}
}

#[allow(dead_code)]
unsafe fn disect<PT: Translate>(pt: PT, virt_addr: x86_64::VirtAddr) {
	match pt.translate(virt_addr) {
		TranslateResult::Mapped {
			frame,
			offset,
			flags,
		} => {
			let phys_addr = frame.start_address() + offset;
			println!("virt_addr: {virt_addr:p}, phys_addr: {phys_addr:p}, flags: {flags:?}");
			match frame {
				MappedFrame::Size4KiB(_) => {
					let page = Page::<BasePageSize>::containing_address(virt_addr);
					println!(
						"p4: {}, p3: {}, p2: {}, p1: {}",
						u16::from(page.p4_index()),
						u16::from(page.p3_index()),
						u16::from(page.p2_index()),
						u16::from(page.p1_index())
					);
				}
				MappedFrame::Size2MiB(_) => {
					let page = Page::<LargePageSize>::containing_address(virt_addr);
					println!(
						"p4: {}, p3: {}, p2: {}",
						u16::from(page.p4_index()),
						u16::from(page.p3_index()),
						u16::from(page.p2_index()),
					);
				}
				MappedFrame::Size1GiB(_) => {
					let page = Page::<HugePageSize>::containing_address(virt_addr);
					println!(
						"p4: {}, p3: {}",
						u16::from(page.p4_index()),
						u16::from(page.p3_index()),
					);
				}
			}
		}
		TranslateResult::NotMapped => todo!(),
		TranslateResult::InvalidFrameAddress(_) => todo!(),
	}
}

#[allow(dead_code)]
unsafe fn print_page_tables(levels: usize) {
	assert!((1..=4).contains(&levels));

	fn print(table: &x86_64::structures::paging::PageTable, level: usize, min_level: usize) {
		for (i, entry) in table
			.iter()
			.enumerate()
			.filter(|(_i, entry)| !entry.is_unused())
		{
			if level <= min_level {
				break;
			}
			let indent = &"        "[0..2 * (4 - level)];
			println!("{indent}L{level} Entry {i}: {entry:?}",);

			if level > min_level && !entry.flags().contains(PageTableEntryFlags::HUGE_PAGE) {
				let phys = entry.frame().unwrap().start_address();
				let virt = x86_64::VirtAddr::new(phys.as_u64());
				let entry_table = unsafe { &*virt.as_mut_ptr() };

				print(entry_table, level - 1, min_level);
			}
		}
	}

	// Recursive
	// let mut recursive_page_table = unsafe { recursive_page_table() };
	// let pt = recursive_page_table.level_4_table();

	// Identity mapped
	let level_4_table_addr = Cr3::read().0.start_address().as_u64();
	let level_4_table_ptr =
		ptr::from_exposed_addr::<PageTable>(level_4_table_addr.try_into().unwrap());
	let pt = unsafe { &*level_4_table_ptr };

	print(pt, 4, 5 - levels);
}
