use align_address::Align;
use free_list::PageRange;
use memory_addresses::VirtAddr;

use crate::arch::riscv64::kernel::get_ram_address;
use crate::arch::riscv64::mm::paging::{HugePageSize, PageSize};
use crate::mm::physicalmem;
use crate::mm::virtualmem::KERNEL_FREE_LIST;

pub fn init() {
	let range = PageRange::new(
		(get_ram_address() + physicalmem::total_memory_size())
			.align_up(HugePageSize::SIZE)
			.as_usize(),
		kernel_heap_end().as_usize(),
	)
	.unwrap();
	unsafe {
		KERNEL_FREE_LIST.lock().deallocate(range).unwrap();
	}
}

/// End of the virtual memory address space reserved for kernel memory (256 GiB).
/// This also marks the start of the virtual memory address space reserved for the task heap.
#[inline]
pub fn kernel_heap_end() -> VirtAddr {
	VirtAddr::new(0x0040_0000_0000)
}
