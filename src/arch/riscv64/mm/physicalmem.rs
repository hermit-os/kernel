use core::convert::TryInto;
use core::sync::atomic::{AtomicUsize, Ordering};

use free_list::{AllocError, FreeList, PageLayout, PageRange};
use hermit_sync::InterruptSpinMutex;

use crate::arch::riscv64::kernel::{get_limit, get_ram_address};
use crate::arch::riscv64::mm::paging::{BasePageSize, PageSize};
use crate::arch::riscv64::mm::PhysAddr;
use crate::mm;

static PHYSICAL_FREE_LIST: InterruptSpinMutex<FreeList<16>> =
	InterruptSpinMutex::new(FreeList::new());
static TOTAL_MEMORY: AtomicUsize = AtomicUsize::new(0);

fn detect_from_limits() -> Result<(), ()> {
	let limit = get_limit();
	if limit == 0 {
		return Err(());
	}

	let range = PageRange::new(
		mm::kernel_end_address().as_usize(),
		get_ram_address().as_usize() + limit,
	)
	.unwrap();
	unsafe {
		PHYSICAL_FREE_LIST.lock().deallocate(range).unwrap();
	}
	TOTAL_MEMORY.store(limit, Ordering::SeqCst);

	Ok(())
}

pub fn init() {
	detect_from_limits().unwrap();
}

pub fn total_memory_size() -> usize {
	TOTAL_MEMORY.load(Ordering::SeqCst)
}

pub fn allocate(size: usize) -> Result<PhysAddr, AllocError> {
	assert!(size > 0);
	assert_eq!(
		size % BasePageSize::SIZE as usize,
		0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE as usize
	);

	let layout = PageLayout::from_size(size).unwrap();

	Ok(PhysAddr(
		PHYSICAL_FREE_LIST
			.lock()
			.allocate(layout)?
			.start()
			.try_into()
			.unwrap(),
	))
}

pub fn allocate_aligned(size: usize, align: usize) -> Result<PhysAddr, AllocError> {
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
		BasePageSize::SIZE as usize
	);

	let layout = PageLayout::from_size_align(size, align).unwrap();

	Ok(PhysAddr(
		PHYSICAL_FREE_LIST
			.lock()
			.allocate(layout)?
			.start()
			.try_into()
			.unwrap(),
	))
}

/// This function must only be called from mm::deallocate!
/// Otherwise, it may fail due to an empty node pool (POOL.maintain() is called in virtualmem::deallocate)
pub fn deallocate(physical_address: PhysAddr, size: usize) {
	assert!(
		physical_address >= PhysAddr(mm::kernel_end_address().as_u64()),
		"Physical address {physical_address:#X} is not >= KERNEL_END_ADDRESS"
	);
	assert!(size > 0);
	assert_eq!(
		size % BasePageSize::SIZE as usize,
		0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE as usize
	);

	let range = PageRange::from_start_len(physical_address.as_usize(), size).unwrap();

	unsafe {
		PHYSICAL_FREE_LIST.lock().deallocate(range).unwrap();
	}
}

pub fn print_information() {
	let free_list = PHYSICAL_FREE_LIST.lock();
	info!("Physical memory free list:\n{free_list}");
}
