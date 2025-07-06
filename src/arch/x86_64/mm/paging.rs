use core::fmt::Debug;
use core::ptr;

use x86_64::registers::control::{Cr0, Cr0Flags, Cr2, Cr3};
#[cfg(feature = "common-os")]
use x86_64::registers::segmentation::SegmentSelector;
pub use x86_64::structures::idt::InterruptStackFrame as ExceptionStackFrame;
use x86_64::structures::idt::PageFaultErrorCode;
pub use x86_64::structures::paging::PageTableFlags as PageTableEntryFlags;
use x86_64::structures::paging::frame::PhysFrameRange;
use x86_64::structures::paging::mapper::{MapToError, MappedFrame, TranslateResult, UnmapError};
use x86_64::structures::paging::page::PageRange;
use x86_64::structures::paging::{
	Mapper, OffsetPageTable, Page, PageTable, PageTableIndex, PhysFrame, RecursivePageTable,
	Size4KiB, Translate,
};

use crate::arch::x86_64::kernel::processor;
use crate::arch::x86_64::mm::{PhysAddr, VirtAddr};
use crate::mm::physicalmem;
use crate::{env, scheduler};

pub trait PageTableEntryFlagsExt {
	fn device(&mut self) -> &mut Self;

	fn normal(&mut self) -> &mut Self;

	#[cfg(feature = "acpi")]
	fn read_only(&mut self) -> &mut Self;

	fn writable(&mut self) -> &mut Self;

	fn execute_disable(&mut self) -> &mut Self;

	#[cfg(feature = "common-os")]
	fn execute_enable(&mut self) -> &mut Self;

	#[cfg(feature = "common-os")]
	fn user(&mut self) -> &mut Self;

	#[expect(dead_code)]
	#[cfg(feature = "common-os")]
	fn kernel(&mut self) -> &mut Self;
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

	#[cfg(feature = "acpi")]
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

	#[cfg(feature = "common-os")]
	fn execute_enable(&mut self) -> &mut Self {
		self.remove(PageTableEntryFlags::NO_EXECUTE);
		self
	}

	#[cfg(feature = "common-os")]
	fn user(&mut self) -> &mut Self {
		self.insert(PageTableEntryFlags::USER_ACCESSIBLE);
		self
	}

	#[cfg(feature = "common-os")]
	fn kernel(&mut self) -> &mut Self {
		self.remove(PageTableEntryFlags::USER_ACCESSIBLE);
		self
	}
}

pub use x86_64::structures::paging::{
	PageSize, Size1GiB as HugePageSize, Size2MiB as LargePageSize, Size4KiB as BasePageSize,
};

/// Returns a mapping of the physical memory where physical address is equal to the virtual address (no offset)
pub unsafe fn identity_mapped_page_table() -> OffsetPageTable<'static> {
	let level_4_table_addr = Cr3::read().0.start_address().as_u64();
	let level_4_table_ptr =
		ptr::with_exposed_provenance_mut::<PageTable>(level_4_table_addr.try_into().unwrap());
	unsafe {
		let level_4_table = level_4_table_ptr.as_mut().unwrap();
		OffsetPageTable::new(level_4_table, x86_64::addr::VirtAddr::new(0x0))
	}
}

/// Translate a virtual memory address to a physical one.
pub fn virtual_to_physical(virtual_address: VirtAddr) -> Option<PhysAddr> {
	let addr = x86_64::VirtAddr::from(virtual_address);

	let translate_result = unsafe { identity_mapped_page_table() }.translate(addr);

	match translate_result {
		TranslateResult::NotMapped | TranslateResult::InvalidFrameAddress(_) => {
			trace!("Uable to determine the physical address of 0x{virtual_address:X}");
			None
		}
		TranslateResult::Mapped { frame, offset, .. } => {
			Some(PhysAddr::new((frame.start_address() + offset).as_u64()))
		}
	}
}

/// Maps a continuous range of pages.
///
/// # Arguments
///
/// * `physical_address` - First physical address to map these pages to
/// * `flags` - Flags from PageTableEntryFlags to set for the page table entry (e.g. WRITABLE or NO_EXECUTE).
///   The PRESENT flags is set automatically.
pub fn map<S>(
	virtual_address: VirtAddr,
	physical_address: PhysAddr,
	count: usize,
	flags: PageTableEntryFlags,
) where
	S: PageSize + Debug,
	for<'a> RecursivePageTable<'a>: Mapper<S>,
	for<'a> OffsetPageTable<'a>: Mapper<S>,
{
	let pages = {
		let start = Page::<S>::containing_address(virtual_address.into());
		let end = start + count as u64;
		Page::range(start, end)
	};

	let frames = {
		let start = PhysFrame::<S>::containing_address(physical_address.into());
		let end = start + count as u64;
		PhysFrame::range(start, end)
	};

	let flags = flags | PageTableEntryFlags::PRESENT;

	trace!("Mapping {pages:?} to {frames:?} with {flags:?}");

	unsafe fn map_pages<M, S>(
		mapper: &mut M,
		pages: PageRange<S>,
		frames: PhysFrameRange<S>,
		flags: PageTableEntryFlags,
	) -> bool
	where
		M: Mapper<S>,
		S: PageSize + Debug,
	{
		let mut frame_allocator = physicalmem::PHYSICAL_FREE_LIST.lock();
		let mut unmapped = false;
		for (page, frame) in pages.zip(frames) {
			// TODO: Require explicit unmaps
			let unmap = mapper.unmap(page);
			if let Ok((_frame, flush)) = unmap {
				unmapped = true;
				flush.flush();
				debug!("Had to unmap page {page:?} before mapping.");
			}
			let map = unsafe { mapper.map_to(page, frame, flags, &mut *frame_allocator) };
			map.unwrap().flush();
		}
		unmapped
	}

	let unmapped = unsafe { map_pages(&mut identity_mapped_page_table(), pages, frames, flags) };

	if unmapped {
		#[cfg(feature = "smp")]
		crate::arch::x86_64::kernel::apic::ipi_tlb_flush();
	}
}

/// Maps `count` pages at address `virt_addr`. If the allocation of a physical memory failed,
/// the number of successful mapped pages are returned as error value.
pub fn map_heap<S>(virt_addr: VirtAddr, count: usize) -> Result<(), usize>
where
	S: PageSize + Debug,
	for<'a> RecursivePageTable<'a>: Mapper<S>,
	for<'a> OffsetPageTable<'a>: Mapper<S>,
{
	let flags = {
		let mut flags = PageTableEntryFlags::empty();
		flags.normal().writable().execute_disable();
		flags
	};

	let virt_addrs = (0..count).map(|n| virt_addr + n as u64 * S::SIZE);

	for (map_counter, virt_addr) in virt_addrs.enumerate() {
		let phys_addr = physicalmem::allocate_aligned(S::SIZE as usize, S::SIZE as usize)
			.map_err(|_| map_counter)?;
		map::<S>(virt_addr, phys_addr, 1, flags);
	}

	Ok(())
}

pub fn identity_map<S>(phys_addr: PhysAddr)
where
	S: PageSize + Debug,
	for<'a> RecursivePageTable<'a>: Mapper<S>,
	for<'a> OffsetPageTable<'a>: Mapper<S>,
{
	let frame = PhysFrame::<S>::from_start_address(phys_addr.into()).unwrap();
	let flags = PageTableEntryFlags::PRESENT
		| PageTableEntryFlags::WRITABLE
		| PageTableEntryFlags::NO_EXECUTE;
	let mut frame_allocator = physicalmem::PHYSICAL_FREE_LIST.lock();
	let mapper_result =
		unsafe { identity_mapped_page_table().identity_map(frame, flags, &mut *frame_allocator) };

	match mapper_result {
		Ok(mapper_flush) => mapper_flush.flush(),
		Err(MapToError::PageAlreadyMapped(current_frame)) => assert_eq!(current_frame, frame),
		Err(MapToError::ParentEntryHugePage) => {
			let page_table = unsafe { identity_mapped_page_table() };
			let virt_addr = VirtAddr::new(frame.start_address().as_u64()).into();
			let phys_addr = frame.start_address();
			assert_eq!(page_table.translate_addr(virt_addr), Some(phys_addr));
		}
		Err(err) => panic!("could not identity-map {frame:?}: {err:?}"),
	}
}

pub fn unmap<S>(virtual_address: VirtAddr, count: usize)
where
	S: PageSize + Debug,
	for<'a> RecursivePageTable<'a>: Mapper<S>,
	for<'a> OffsetPageTable<'a>: Mapper<S>,
{
	trace!("Unmapping virtual address {virtual_address:p} ({count} pages)");

	let first_page = Page::<S>::containing_address(virtual_address.into());
	let last_page = first_page + count as u64;
	let range = Page::range(first_page, last_page);

	for page in range {
		let unmap_result = unsafe { identity_mapped_page_table() }.unmap(page);
		match unmap_result {
			Ok((_frame, flush)) => flush.flush(),
			// FIXME: Some sentinel pages around stacks are supposed to be unmapped.
			// We should handle this case there instead of here.
			Err(UnmapError::PageNotMapped) => {
				debug!("Tried to unmap {page:?}, which was not mapped.");
			}
			Err(err) => panic!("{err:?}"),
		}
	}
}

#[cfg(not(feature = "common-os"))]
pub(crate) extern "x86-interrupt" fn page_fault_handler(
	stack_frame: ExceptionStackFrame,
	error_code: PageFaultErrorCode,
) {
	error!("Page fault (#PF)!");
	error!("page_fault_linear_address = {:p}", Cr2::read().unwrap());
	error!("error_code = {error_code:?}");
	error!("fs = {:#X}", processor::readfs());
	error!("gs = {:#X}", processor::readgs());
	error!("stack_frame = {stack_frame:#?}");
	scheduler::abort();
}

#[cfg(feature = "common-os")]
pub(crate) extern "x86-interrupt" fn page_fault_handler(
	mut stack_frame: ExceptionStackFrame,
	error_code: PageFaultErrorCode,
) {
	unsafe {
		if stack_frame.as_mut().read().code_segment != SegmentSelector(0x08) {
			core::arch::asm!("swapgs", options(nostack));
		}
	}
	error!("Page fault (#PF)!");
	error!("page_fault_linear_address = {:p}", Cr2::read().unwrap());
	error!("error_code = {error_code:?}");
	error!("fs = {:#X}", processor::readfs());
	error!("gs = {:#X}", processor::readgs());
	error!("stack_frame = {stack_frame:#?}");
	scheduler::abort();
}

pub fn init() {
	make_p4_writable();
}

fn make_p4_writable() {
	debug!("Making P4 table writable");

	if !env::is_uefi() {
		return;
	}

	let mut pt = unsafe { identity_mapped_page_table() };

	let p4_page = {
		let (p4_frame, _) = Cr3::read_raw();
		let p4_addr = x86_64::VirtAddr::new(p4_frame.start_address().as_u64());
		Page::<Size4KiB>::from_start_address(p4_addr).unwrap()
	};

	let TranslateResult::Mapped { frame, flags, .. } = pt.translate(p4_page.start_address()) else {
		unreachable!()
	};

	let make_writable = || unsafe {
		let flags = flags | PageTableEntryFlags::WRITABLE;
		match frame {
			MappedFrame::Size1GiB(_) => pt.set_flags_p3_entry(p4_page, flags).unwrap().ignore(),
			MappedFrame::Size2MiB(_) => pt.set_flags_p2_entry(p4_page, flags).unwrap().ignore(),
			MappedFrame::Size4KiB(_) => pt.update_flags(p4_page, flags).unwrap().ignore(),
		}
	};

	unsafe fn without_protect<F, R>(f: F) -> R
	where
		F: FnOnce() -> R,
	{
		let cr0 = Cr0::read();
		if cr0.contains(Cr0Flags::WRITE_PROTECT) {
			unsafe { Cr0::write(cr0 - Cr0Flags::WRITE_PROTECT) }
		}
		let ret = f();
		if cr0.contains(Cr0Flags::WRITE_PROTECT) {
			unsafe { Cr0::write(cr0) }
		}
		ret
	}

	unsafe { without_protect(make_writable) }
}

pub fn init_page_tables() {}

#[allow(dead_code)]
unsafe fn print_page_table_entries(page_table_indices: &[PageTableIndex]) {
	assert!(page_table_indices.len() <= 4);

	let identity_mapped_page_table = unsafe { identity_mapped_page_table() };
	let mut pt = identity_mapped_page_table.level_4_table();

	for (i, page_table_index) in page_table_indices.iter().copied().enumerate() {
		let level = 4 - i;
		let entry = &pt[page_table_index];

		let indent = &"        "[0..2 * i];
		let page_table_index = u16::from(page_table_index);
		println!("{indent}L{level} Entry {page_table_index}: {entry:?}");

		if entry.is_unused() {
			break;
		}

		let phys = entry.addr();
		let virt = x86_64::VirtAddr::new(phys.as_u64());
		pt = unsafe { &*virt.as_mut_ptr() };
	}
}

#[allow(dead_code)]
pub(crate) unsafe fn print_page_tables(levels: usize) {
	assert!((1..=4).contains(&levels));

	fn print(table: &x86_64::structures::paging::PageTable, level: usize, min_level: usize) {
		for (i, entry) in table
			.iter()
			.enumerate()
			.filter(|(_i, entry)| !entry.is_unused())
		{
			if level < min_level {
				break;
			}
			let indent = &"        "[0..2 * (4 - level)];
			println!("{indent}L{level} Entry {i}: {entry:?}");

			if level > min_level && !entry.flags().contains(PageTableEntryFlags::HUGE_PAGE) {
				let phys = entry.frame().unwrap().start_address();
				let virt = x86_64::VirtAddr::new(phys.as_u64());
				let entry_table = unsafe { &*virt.as_mut_ptr() };

				print(entry_table, level - 1, min_level);
			}
		}
	}

	let identity_mapped_page_table = unsafe { identity_mapped_page_table() };
	let pt = identity_mapped_page_table.level_4_table();

	print(pt, 4, 5 - levels);
}
