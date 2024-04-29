use free_list::{AllocError, FreeList, PageLayout, PageRange};
use hermit_sync::InterruptTicketMutex;

use crate::arch::x86_64::mm::paging::{BasePageSize, PageSize};
use crate::arch::x86_64::mm::VirtAddr;
use crate::mm;

static KERNEL_FREE_LIST: InterruptTicketMutex<FreeList<16>> =
	InterruptTicketMutex::new(FreeList::new());

pub fn init() {
	let range = PageRange::new(
		mm::kernel_end_address().as_usize(),
		kernel_heap_end().as_usize(),
	)
	.unwrap();
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

	Ok(VirtAddr(
		KERNEL_FREE_LIST
			.lock()
			.allocate(layout)?
			.start()
			.try_into()
			.unwrap(),
	))
}

#[cfg(not(feature = "newlib"))]
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

	Ok(VirtAddr(
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
		virtual_address >= VirtAddr(mm::kernel_end_address().as_u64()),
		"Virtual address {virtual_address:p} is not >= KERNEL_END_ADDRESS"
	);
	assert!(
		virtual_address < kernel_heap_end(),
		"Virtual address {virtual_address:p} is not < kernel_heap_end()"
	);
	assert_eq!(
		virtual_address % BasePageSize::SIZE,
		0,
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

	let range = PageRange::from_start_len(virtual_address.as_usize(), size).unwrap();

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
		virtual_address < kernel_heap_end(),
		"Virtual address {:#X} is not < kernel_heap_end()",
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

/// End of the virtual memory address space reserved for kernel memory.
/// This also marks the start of the virtual memory address space reserved for the task heap.
/// In case of pure rust applications, we don't have a task heap.
#[cfg(all(not(feature = "common-os"), not(feature = "newlib")))]
#[inline]
pub const fn kernel_heap_end() -> VirtAddr {
	VirtAddr(0x8000_0000_0000u64)
}

#[cfg(all(feature = "common-os", not(feature = "newlib")))]
#[inline]
pub const fn kernel_heap_end() -> VirtAddr {
	VirtAddr(0x100_0000_0000u64)
}

#[cfg(all(not(feature = "common-os"), feature = "newlib"))]
#[inline]
pub const fn kernel_heap_end() -> VirtAddr {
	VirtAddr(0x1_0000_0000u64)
}
