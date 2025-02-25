use align_address::Align;
use free_list::{AllocError, FreeList, PageLayout, PageRange};
use hermit_sync::InterruptTicketMutex;

use crate::arch::x86_64::mm::VirtAddr;
use crate::arch::x86_64::mm::paging::{BasePageSize, PageSize};
use crate::{env, mm};

static KERNEL_FREE_LIST: InterruptTicketMutex<FreeList<16>> =
	InterruptTicketMutex::new(FreeList::new());

pub fn init() {
	let range = if env::is_uefi() {
		let fdt = env::fdt().unwrap();

		let biggest_region = fdt
			.find_all_nodes("/memory")
			.map(|m| m.reg().unwrap().next().unwrap())
			.max_by_key(|m| m.size.unwrap())
			.unwrap();

		PageRange::from_start_len(
			biggest_region.starting_address.addr(),
			biggest_region.size.unwrap(),
		)
		.unwrap()
	} else {
		PageRange::new(
			mm::kernel_end_address().as_usize(),
			kernel_heap_end().as_usize() + 1,
		)
		.unwrap()
	};

	unsafe {
		KERNEL_FREE_LIST.lock().deallocate(range).unwrap();
	}
}

pub fn allocate(size: usize) -> Result<VirtAddr, AllocError> {
	assert!(size > 0);
	assert_eq!(
		size % BasePageSize::SIZE as usize,
		0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE
	);

	let layout = PageLayout::from_size(size).unwrap();

	Ok(VirtAddr::new(
		KERNEL_FREE_LIST
			.lock()
			.allocate(layout)?
			.start()
			.try_into()
			.unwrap(),
	))
}

pub fn allocate_aligned(size: usize, align: usize) -> Result<VirtAddr, AllocError> {
	assert!(size > 0);
	assert!(align > 0);
	assert_eq!(
		size % align,
		0,
		"Size {size:#X} is not a multiple of the given alignment {align:#X}"
	);
	assert_eq!(
		align % BasePageSize::SIZE as usize,
		0,
		"Alignment {:#X} is not a multiple of {:#X}",
		align,
		BasePageSize::SIZE
	);

	let layout = PageLayout::from_size_align(size, align).unwrap();

	Ok(VirtAddr::new(
		KERNEL_FREE_LIST
			.lock()
			.allocate(layout)?
			.start()
			.try_into()
			.unwrap(),
	))
}

pub fn deallocate(virtual_address: VirtAddr, size: usize) {
	assert!(
		virtual_address <= kernel_heap_end(),
		"Virtual address {virtual_address:p} is not <= kernel_heap_end()"
	);
	assert!(
		virtual_address.is_aligned_to(BasePageSize::SIZE),
		"Virtual address {:p} is not a multiple of {:#X}",
		virtual_address,
		BasePageSize::SIZE
	);
	assert!(size > 0);
	assert_eq!(
		size % BasePageSize::SIZE as usize,
		0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE
	);

	let range = PageRange::from_start_len(virtual_address.as_u64() as usize, size).unwrap();

	unsafe {
		KERNEL_FREE_LIST.lock().deallocate(range).unwrap();
	}
}

/*pub fn reserve(virtual_address: VirtAddr, size: usize) {
	assert!(
		virtual_address >= VirtAddr(mm::kernel_end_address().as_u64()),
		"Virtual address {:#X} is not >= KERNEL_END_ADDRESS",
		virtual_address
	);
	assert!(
		virtual_address <= kernel_heap_end(),
		"Virtual address {:#X} is not <= kernel_heap_end()",
		virtual_address
	);
	assert_eq!(
		virtual_address % BasePageSize::SIZE,
		0,
		"Virtual address {:#X} is not a multiple of {:#X}",
		virtual_address,
		BasePageSize::SIZE
	);
	assert!(size > 0);
	assert_eq!(
		size % BasePageSize::SIZE as usize,
		0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE
	);

	let result = KERNEL_FREE_LIST
		.lock()
		.reserve(virtual_address.as_usize(), size);
	assert!(
		result.is_ok(),
		"Could not reserve {:#X} bytes of virtual memory at {:#X}",
		size,
		virtual_address
	);
}*/

pub fn print_information() {
	let free_list = KERNEL_FREE_LIST.lock();
	info!("Virtual memory free list:\n{free_list}");
}

/// End of the virtual memory address space reserved for kernel memory (inclusive).
/// The virtual memory address space reserved for the task heap starts after this.
#[inline]
pub fn kernel_heap_end() -> VirtAddr {
	use x86_64::structures::paging::PageTableIndex;

	let p4_index = if cfg!(feature = "common-os") {
		PageTableIndex::new(1)
	} else {
		PageTableIndex::new(256)
	};

	let addr = u64::from(p4_index) << 39;
	assert_eq!(VirtAddr::new_truncate(addr).p4_index(), p4_index);

	VirtAddr::new_truncate(addr - 1)
}
