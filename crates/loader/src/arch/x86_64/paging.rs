use core::fmt::Debug;
use core::ptr;

use log::warn;
use x86_64::VirtAddr;
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::mapper::{CleanUp, MapToError};
use x86_64::structures::paging::{
	Mapper, OffsetPageTable, Page, PageSize, PageTable, PageTableFlags, PhysFrame, Translate,
};

use super::physicalmem::PhysAlloc;

pub fn map<S>(virtual_address: usize, physical_address: usize, count: usize, flags: PageTableFlags)
where
	S: PageSize + Debug,
	for<'a> OffsetPageTable<'a>: Mapper<S>,
{
	let pages = {
		let start = Page::<S>::containing_address(x86_64::VirtAddr::new(virtual_address as u64));
		let end = start + count as u64;
		Page::range(start, end)
	};

	let frames = {
		let start =
			PhysFrame::<S>::containing_address(x86_64::PhysAddr::new(physical_address as u64));
		let end = start + count as u64;
		PhysFrame::range(start, end)
	};

	warn!(
		"Mapping {count} {size} pages from {from_start:p}..{from_end:p} to {to_start:p}..{to_end:p}",
		count = (pages.end.start_address() - pages.start.start_address()) / S::SIZE,
		size = S::DEBUG_STR,
		from_start = pages.start.start_address(),
		from_end = pages.end.start_address(),
		to_start = frames.start.start_address(),
		to_end = frames.end.start_address(),
	);

	let flags = flags | PageTableFlags::PRESENT;
	let mut table = unsafe { identity_mapped_page_table() };

	for (page, frame) in pages.zip(frames) {
		let mapper_result = unsafe { table.map_to(page, frame, flags, &mut PhysAlloc) };
		match mapper_result {
			Ok(mapper_flush) => mapper_flush.flush(),
			Err(MapToError::PageAlreadyMapped(current_frame)) => assert_eq!(current_frame, frame),
			Err(MapToError::ParentEntryHugePage) => {
				let current_addr = table.translate_addr(page.start_address()).unwrap();
				let expected_addr = frame.start_address();
				assert_eq!(current_addr, expected_addr);
			}
			Err(err) => panic!("could not map {frame:?}: {err:?}"),
		}
	}
}

#[cfg(feature = "multiboot")]
pub fn map_range<S>(
	virtual_start: usize,
	phys_start: usize,
	phys_end: usize,
	mut flags: PageTableFlags,
) where
	S: PageSize + Debug,
	for<'a> OffsetPageTable<'a>: Mapper<S>,
{
	let first_page = Page::<S>::containing_address(x86_64::VirtAddr::new(virtual_start as u64));
	let first_frame = PhysFrame::containing_address(x86_64::PhysAddr::new(phys_start as u64));
	let last_frame = PhysFrame::containing_address(x86_64::PhysAddr::new(phys_end as u64));
	warn!(
		"Mapping {size} pages starting from {from_start:p} to frames {to_start:p}..{to_end:p}",
		size = S::DEBUG_STR,
		from_start = first_page.start_address(),
		to_start = first_frame.start_address(),
		to_end = last_frame.start_address()
	);
	flags |= PageTableFlags::PRESENT;
	let mut table = unsafe { identity_mapped_page_table() };
	let page_range = core::iter::successors(Some(first_page), |page| Some(*page + 1u64));
	let frame_range = PhysFrame::<S>::range(first_frame, last_frame);
	for (page, frame) in core::iter::zip(page_range, frame_range) {
		let mapper_result = unsafe { table.map_to(page, frame, flags, &mut PhysAlloc) };
		match mapper_result {
			Ok(mapper_flush) => mapper_flush.flush(),
			Err(MapToError::PageAlreadyMapped(current_frame)) => assert_eq!(current_frame, frame),
			Err(MapToError::ParentEntryHugePage) => {
				let current_addr = table.translate_addr(page.start_address()).unwrap();
				let expected_addr = frame.start_address();
				assert_eq!(current_addr, expected_addr);
			}
			Err(err) => panic!("could not map {frame:?}: {err:?}"),
		}
	}
}

pub fn clean_up() {
	let mut table = unsafe { identity_mapped_page_table() };

	unsafe { table.clean_up(&mut PhysAlloc) }
}

unsafe fn identity_mapped_page_table() -> OffsetPageTable<'static> {
	let level_4_table_addr = Cr3::read().0.start_address().as_u64();
	let level_4_table_addr = usize::try_from(level_4_table_addr).unwrap();
	let level_4_table_ptr = ptr::with_exposed_provenance_mut::<PageTable>(level_4_table_addr);
	let level_4_table = unsafe { level_4_table_ptr.as_mut().unwrap() };
	let phys_offset = VirtAddr::new(0x0);
	unsafe { OffsetPageTable::new(level_4_table, phys_offset) }
}
