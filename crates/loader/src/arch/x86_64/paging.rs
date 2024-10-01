use core::fmt::Debug;

use log::warn;
use x86_64::structures::paging::mapper::CleanUp;
use x86_64::structures::paging::{
	Mapper, Page, PageSize, PageTableFlags, PhysFrame, RecursivePageTable,
};

use super::physicalmem::PhysAlloc;

pub fn map<S>(virtual_address: usize, physical_address: usize, count: usize, flags: PageTableFlags)
where
	S: PageSize + Debug,
	RecursivePageTable<'static>: Mapper<S>,
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
	let mut table = unsafe { recursive_page_table() };

	for (page, frame) in pages.zip(frames) {
		unsafe {
			table
				.map_to(page, frame, flags, &mut PhysAlloc)
				.unwrap()
				.flush();
		}
	}
}

#[cfg(all(target_arch = "x86_64", not(feature = "fc")))]
pub fn map_range<S>(
	virtual_start: usize,
	phys_start: usize,
	phys_end: usize,
	mut flags: PageTableFlags,
) where
	S: PageSize + Debug,
	RecursivePageTable<'static>: Mapper<S>,
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
	let mut table = unsafe { recursive_page_table() };
	let page_range = core::iter::successors(Some(first_page), |page| Some(*page + 1u64));
	let frame_range = PhysFrame::<S>::range(first_frame, last_frame);
	for (page, frame) in core::iter::zip(page_range, frame_range) {
		unsafe {
			table
				.map_to(page, frame, flags, &mut PhysAlloc)
				.unwrap()
				.flush();
		}
	}
}

pub fn clean_up() {
	let mut table = unsafe { recursive_page_table() };

	unsafe { table.clean_up(&mut PhysAlloc) }
}

unsafe fn recursive_page_table() -> RecursivePageTable<'static> {
	let level_4_table_addr = 0xFFFF_FFFF_FFFF_F000_usize;
	let level_4_table_ptr = sptr::from_exposed_addr_mut(level_4_table_addr);
	unsafe {
		let level_4_table = &mut *(level_4_table_ptr);
		RecursivePageTable::new(level_4_table).unwrap()
	}
}
