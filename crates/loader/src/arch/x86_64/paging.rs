use core::fmt::Debug;

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

	log::warn!(
		"Mapping {count} {size} pages from {from_start:p}..{from_end:p} to {to_start:p}..{to_end:p}",
		count = (pages.end.start_address() - pages.start.start_address()) / S::SIZE,
		size = S::SIZE_AS_DEBUG_STR,
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
