pub(crate) mod paging;

use memory_addresses::arch::x86_64::{PhysAddr, VirtAddr};
#[cfg(feature = "common-os")]
use x86_64::structures::paging::{PageSize, Size4KiB as BasePageSize};

pub use self::paging::init_page_tables;
#[cfg(feature = "common-os")]
use crate::arch::mm::paging::{PageTableEntryFlags, PageTableEntryFlagsExt};

#[cfg(feature = "common-os")]
pub fn create_new_root_page_table() -> usize {
	use x86_64::registers::control::Cr3;

	let physaddr = crate::mm::physicalmem::allocate_aligned(
		BasePageSize::SIZE as usize,
		BasePageSize::SIZE as usize,
	)
	.unwrap();
	let virtaddr = crate::mm::virtualmem::allocate_aligned(
		2 * BasePageSize::SIZE as usize,
		BasePageSize::SIZE as usize,
	)
	.unwrap();
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
	crate::mm::virtualmem::deallocate(virtaddr, 2 * BasePageSize::SIZE as usize);

	physaddr.as_usize()
}

pub fn init() {
	paging::init();
	crate::mm::physicalmem::init();
	unsafe {
		paging::log_page_tables();
	}
	crate::mm::virtualmem::init();

	#[cfg(feature = "common-os")]
	{
		use x86_64::registers::control::Cr3;

		let (frame, _flags) = Cr3::read();
		crate::scheduler::BOOT_ROOT_PAGE_TABLE
			.set(frame.start_address().as_u64().try_into().unwrap())
			.unwrap();
	}
}
