use core::alloc::AllocError;
use core::sync::atomic::{AtomicUsize, Ordering};

use hermit_sync::InterruptTicketMutex;

use crate::arch::aarch64::kernel::get_limit;
use crate::arch::aarch64::mm::paging::{BasePageSize, PageSize};
use crate::arch::aarch64::mm::PhysAddr;
use crate::mm;
use crate::mm::freelist::{FreeList, FreeListEntry};

static PHYSICAL_FREE_LIST: InterruptTicketMutex<FreeList> =
	InterruptTicketMutex::new(FreeList::new());
static TOTAL_MEMORY: AtomicUsize = AtomicUsize::new(0);

fn detect_from_limits() -> Result<(), ()> {
	let limit = get_limit();
	if limit == 0 {
		return Err(());
	}

	let entry = FreeListEntry {
		start: mm::kernel_end_address().as_usize(),
		end: limit,
	};
	TOTAL_MEMORY.store(
		limit - mm::kernel_end_address().as_usize(),
		Ordering::SeqCst,
	);
	PHYSICAL_FREE_LIST.lock().push(entry);

	Ok(())
}

pub fn init() {
	detect_from_limits().expect("Unable to determine physical address space!");
}

pub fn total_memory_size() -> usize {
	TOTAL_MEMORY.load(Ordering::SeqCst)
}

pub fn init_page_tables() {}

pub fn allocate(size: usize) -> Result<PhysAddr, AllocError> {
	assert!(size > 0);
	assert_eq!(
		size % BasePageSize::SIZE as usize,
		0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE
	);

	Ok(PhysAddr(
		PHYSICAL_FREE_LIST
			.lock()
			.allocate(size, None)?
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
		BasePageSize::SIZE
	);

	Ok(PhysAddr(
		PHYSICAL_FREE_LIST
			.lock()
			.allocate(size, Some(align))?
			.try_into()
			.unwrap(),
	))
}

/// This function must only be called from mm::deallocate!
/// Otherwise, it may fail due to an empty node pool (POOL.maintain() is called in virtualmem::deallocate)
pub fn deallocate(physical_address: PhysAddr, size: usize) {
	assert!(
		physical_address >= PhysAddr(mm::kernel_end_address().as_u64()),
		"Physical address {physical_address:p} is not >= KERNEL_END_ADDRESS"
	);
	assert!(size > 0);
	assert_eq!(
		size % BasePageSize::SIZE as usize,
		0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE
	);

	PHYSICAL_FREE_LIST
		.lock()
		.deallocate(physical_address.as_usize(), size);
}

pub fn print_information() {
	PHYSICAL_FREE_LIST
		.lock()
		.print_information(" PHYSICAL MEMORY FREE LIST ");
}
