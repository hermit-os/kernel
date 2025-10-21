pub(crate) mod paging;

use memory_addresses::arch::x86_64::{PhysAddr, VirtAddr};
#[cfg(feature = "common-os")]
use x86_64::structures::paging::{PageSize, Size4KiB as BasePageSize};

pub use self::paging::init_page_tables;
#[cfg(feature = "common-os")]
use crate::arch::mm::paging::{PageTableEntryFlags, PageTableEntryFlagsExt};
use crate::mm::{FrameAlloc, PageAlloc, PageRangeAllocator};

#[cfg(feature = "common-os")]
pub fn create_new_root_page_table() -> usize {
	use free_list::{PageLayout, PageRange};
	use x86_64::registers::control::Cr3;

	use crate::mm::{FrameAlloc, PageAlloc, PageRangeAllocator};

	let layout = PageLayout::from_size(BasePageSize::SIZE as usize).unwrap();
	let frame_range = FrameAlloc::allocate(layout).unwrap();
	let physaddr = PhysAddr::from(frame_range.start());

	let layout = PageLayout::from_size(2 * BasePageSize::SIZE as usize).unwrap();
	let page_range = PageAlloc::allocate(layout).unwrap();
	let virtaddr = VirtAddr::from(page_range.start());
	let mut flags = PageTableEntryFlags::empty();
	flags.normal().writable();

	let entry: u64 = unsafe {
		let (frame, _flags) = Cr3::read();
		paging::map::<BasePageSize>(virtaddr, frame.start_address().into(), 1, flags);
		let entry: &u64 = &*virtaddr.as_ptr();

		*entry
	};

	let slice_addr = virtaddr + BasePageSize::SIZE;
	paging::map::<BasePageSize>(slice_addr, physaddr, 1, flags);

	unsafe {
		let pml4 = core::slice::from_raw_parts_mut(slice_addr.as_mut_ptr(), 512);

		// clear PML4
		for elem in pml4.iter_mut() {
			*elem = 0;
		}

		// copy first element and the self reference
		pml4[0] = entry;
		// create self reference
		pml4[511] = physaddr.as_u64() + 0x3; // PG_PRESENT | PG_RW
	};

	paging::unmap::<BasePageSize>(virtaddr, 2);
	let range =
		PageRange::from_start_len(virtaddr.as_usize(), 2 * BasePageSize::SIZE as usize).unwrap();
	unsafe {
		PageAlloc::deallocate(range);
	}

	physaddr.as_usize()
}

pub fn init() {
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
