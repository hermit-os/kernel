use core::alloc::AllocError;
use core::convert::TryInto;

use align_address::Align;
use hermit_sync::InterruptSpinMutex;

use crate::arch::riscv64::kernel::get_ram_address;
use crate::arch::riscv64::mm::paging::{BasePageSize, HugePageSize, PageSize};
use crate::arch::riscv64::mm::{physicalmem, PhysAddr, VirtAddr};
use crate::mm;
use crate::mm::freelist::{FreeList, FreeListEntry};

static KERNEL_FREE_LIST: InterruptSpinMutex<FreeList> = InterruptSpinMutex::new(FreeList::new());

/// End of the virtual memory address space reserved for kernel memory (256 GiB).
/// This also marks the start of the virtual memory address space reserved for the task heap.
const KERNEL_VIRTUAL_MEMORY_END: VirtAddr = VirtAddr(0x4000000000);

pub fn init() {
	let entry = FreeListEntry {
		start: (get_ram_address() + PhysAddr(physicalmem::total_memory_size() as u64))
			.as_usize()
			.align_up(HugePageSize::SIZE as usize),
		end: KERNEL_VIRTUAL_MEMORY_END.as_usize(),
	};
	KERNEL_FREE_LIST.lock().push(entry);
}

pub fn allocate(size: usize) -> Result<VirtAddr, AllocError> {
	assert!(size > 0);
	assert_eq!(
		size % BasePageSize::SIZE as usize,
		0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE as usize
	);

	Ok(VirtAddr(
		KERNEL_FREE_LIST
			.lock()
			.allocate(size, None)?
			.try_into()
			.unwrap(),
	))
}

pub fn allocate_aligned(size: usize, alignment: usize) -> Result<VirtAddr, AllocError> {
	assert!(size > 0);
	assert!(alignment > 0);
	assert_eq!(
		size % alignment,
		0,
		"Size {size:#X} is not a multiple of the given alignment {alignment:#X}",
	);
	assert_eq!(
		alignment % BasePageSize::SIZE as usize,
		0,
		"Alignment {:#X} is not a multiple of {:#X}",
		alignment,
		BasePageSize::SIZE as usize
	);

	Ok(VirtAddr(
		KERNEL_FREE_LIST
			.lock()
			.allocate(size, Some(alignment))?
			.try_into()
			.unwrap(),
	))
}

pub fn deallocate(virtual_address: VirtAddr, size: usize) {
	assert!(
		virtual_address >= mm::kernel_end_address(),
		"Virtual address {virtual_address:#X} is not >= KERNEL_END_ADDRESS"
	);
	assert!(
		virtual_address < KERNEL_VIRTUAL_MEMORY_END,
		"Virtual address {virtual_address:#X} is not < KERNEL_VIRTUAL_MEMORY_END",
	);
	assert_eq!(
		virtual_address % BasePageSize::SIZE as usize,
		0,
		"Virtual address {:#X} is not a multiple of {:#X}",
		virtual_address,
		BasePageSize::SIZE as usize
	);
	assert!(size > 0);
	assert_eq!(
		size % BasePageSize::SIZE as usize,
		0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE as usize
	);

	KERNEL_FREE_LIST
		.lock()
		.deallocate(virtual_address.as_usize(), size);
}

/*pub fn reserve(virtual_address: VirtAddr, size: usize) {
	assert!(
		virtual_address >= mm::kernel_end_address(),
		"Virtual address {:#X} is not >= KERNEL_END_ADDRESS",
		virtual_address
	);
	assert!(
		virtual_address < KERNEL_VIRTUAL_MEMORY_END,
		"Virtual address {:#X} is not < KERNEL_VIRTUAL_MEMORY_END",
		virtual_address
	);
	assert_eq!(
		virtual_address % BasePageSize::SIZE as usize,
		0,
		"Virtual address {:#X} is not a multiple of {:#X}",
		virtual_address,
		BasePageSize::SIZE as usize
	);
	assert!(size > 0);
	assert_eq!(
		size % BasePageSize::SIZE as usize,
		0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE as usize
	);

	let result = KERNEL_FREE_LIST.lock().reserve(virtual_address.as_usize(), size);
	assert!(
		result.is_ok(),
		"Could not reserve {:#X} bytes of virtual memory at {:#X}",
		size,
		virtual_address
	);
}*/

pub fn print_information() {
	KERNEL_FREE_LIST
		.lock()
		.print_information(" KERNEL VIRTUAL MEMORY FREE LIST ");
}
