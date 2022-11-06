use core::fmt::Debug;
use core::ptr;

use x86_64::instructions::tlb;
use x86_64::structures::paging::mapper::UnmapError;
use x86_64::structures::paging::{
	Mapper, Page, PageTableIndex, PhysFrame, RecursivePageTable, Size2MiB,
};

#[cfg(feature = "smp")]
use crate::arch::x86_64::kernel::apic;
use crate::arch::x86_64::mm::physicalmem;
use crate::arch::x86_64::mm::{PhysAddr, VirtAddr};
use crate::env;
use crate::mm;

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

pub use x86_64::structures::paging::PageSize;
pub use x86_64::structures::paging::Size1GiB as HugePageSize;
pub use x86_64::structures::paging::Size2MiB as LargePageSize;
pub use x86_64::structures::paging::Size4KiB as BasePageSize;

/// Translate a virtual memory address to a physical one.
pub fn virtual_to_physical(virtual_address: VirtAddr) -> Option<PhysAddr> {
	use x86_64::structures::paging::mapper::Translate;

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
	RecursivePageTable<'static>: Mapper<S>,
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

	for (page, frame) in pages.zip(frames) {
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

	#[cfg(feature = "smp")]
	apic::ipi_tlb_flush();
}

pub fn map_heap<S: PageSize>(virt_addr: VirtAddr, count: usize)
where
	S: PageSize + Debug,
	RecursivePageTable<'static>: Mapper<S>,
{
	let flags = {
		let mut flags = PageTableEntryFlags::empty();
		flags.normal().writable().execute_disable();
		flags
	};

	let virt_addrs = (0..count).map(|n| virt_addr + n * S::SIZE as usize);

	for virt_addr in virt_addrs {
		let phys_addr = physicalmem::allocate_aligned(S::SIZE as usize, S::SIZE as usize).unwrap();
		map::<S>(virt_addr, phys_addr, 1, flags);
	}
}

unsafe fn recursive_page_table() -> RecursivePageTable<'static> {
	let level_4_table_addr = 0xFFFF_FFFF_FFFF_F000;
	let level_4_table_ptr = ptr::from_exposed_addr_mut(level_4_table_addr);
	unsafe {
		let level_4_table = &mut *(level_4_table_ptr);
		RecursivePageTable::new(level_4_table).unwrap()
	}
}

pub fn unmap<S>(virtual_address: VirtAddr, count: usize)
where
	S: PageSize + Debug,
	RecursivePageTable<'static>: Mapper<S>,
{
	trace!(
		"Unmapping virtual address {:#X} ({} pages)",
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

#[cfg(feature = "acpi")]
pub fn identity_map<S>(frame: PhysFrame<S>)
where
	S: PageSize + Debug,
	RecursivePageTable<'static>: Mapper<S>,
{
	assert!(
		frame.start_address().as_u64() < mm::kernel_start_address().0,
		"Address {:#X} to be identity-mapped is not below Kernel start address",
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

#[inline]
pub fn get_application_page_size() -> usize {
	LargePageSize::SIZE as usize
}

pub fn init() {}

pub fn init_page_tables() {
	if env::is_uhyve() {
		// Uhyve identity-maps the first Gibibyte of memory (512 page table entries * 2MiB pages)
		// We now unmap all memory after the kernel image, so that we can remap it ourselves later for the heap.
		// Ideally, uhyve would only map as much memory as necessary, but this requires a hermit-entry ABI jump.
		// See https://github.com/hermitcore/uhyve/issues/426
		let kernel_end_addr = x86_64::VirtAddr::new(mm::kernel_end_address().as_u64());
		let start_page = Page::<Size2MiB>::from_start_address(kernel_end_addr).unwrap();
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
