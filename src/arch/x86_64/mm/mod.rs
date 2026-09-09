pub(crate) mod paging;

#[cfg(feature = "common-os")]
use core::slice;

use memory_addresses::arch::x86_64::{PhysAddr, VirtAddr};
#[cfg(feature = "common-os")]
use x86_64::structures::paging::{PageSize, Size4KiB as BasePageSize};

#[cfg(feature = "common-os")]
use crate::arch::mm::paging::{PageTableEntryFlags, PageTableEntryFlagsExt};
use crate::mm::{FrameAlloc, PageAlloc, PageRangeAllocator};

#[cfg(feature = "common-os")]
pub fn create_new_root_page_table() -> usize {
	use free_list::PageLayout;
	use x86_64::registers::control::Cr3;

	use crate::mm::MappedPageBox;

	let layout = PageLayout::from_size(BasePageSize::SIZE as usize).unwrap();
	let frame_range = FrameAlloc::allocate(layout).unwrap();
	let physaddr = PhysAddr::from(frame_range.start());

	let mut flags = PageTableEntryFlags::empty();
	flags.normal().writable();

	let entry: u64 = unsafe {
		let (frame, _flags) = Cr3::read();
		let page_range =
			MappedPageBox::map_phys(frame.start_address().into(), layout, flags).unwrap();
		let entry: &u64 = &*VirtAddr::from(page_range.pages().start()).as_ptr();

		*entry
	};

	let page_range = unsafe { MappedPageBox::map_phys(physaddr, layout, flags).unwrap() };
	let slice_addr = VirtAddr::from(page_range.pages().start());

	unsafe {
		let pml4 = slice::from_raw_parts_mut(slice_addr.as_mut_ptr(), 512);

		// clear PML4
		for elem in pml4.iter_mut() {
			*elem = 0;
		}

		// copy first element and the self reference
		pml4[0] = entry;
		// create self reference
		pml4[511] = physaddr.as_u64() + 0x3; // PG_PRESENT | PG_RW
	};

	physaddr.as_usize()
}

pub unsafe fn init() {
	paging::init();
	unsafe {
		FrameAlloc::init();
	}
	unsafe {
		paging::log_page_tables();
	}
	unsafe {
		PageAlloc::init();
	}

	#[cfg(feature = "common-os")]
	{
		use x86_64::registers::control::Cr3;

		let (frame, _flags) = Cr3::read();
		crate::scheduler::BOOT_ROOT_PAGE_TABLE
			.set(frame.start_address().as_u64().try_into().unwrap())
			.unwrap();
	}
}
