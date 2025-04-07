use free_list::PageRange;
use memory_addresses::VirtAddr;

use crate::mm::virtualmem::KERNEL_FREE_LIST;

pub fn init() {
	let range = PageRange::new(
		kernel_heap_end().as_usize() / 2,
		kernel_heap_end().as_usize(),
	)
	.unwrap();
	unsafe {
		KERNEL_FREE_LIST.lock().deallocate(range).unwrap();
	}
}

/// End of the virtual memory address space reserved for kernel memory (4 GiB).
/// This also marks the start of the virtual memory address space reserved for the task heap.
#[inline]
pub fn kernel_heap_end() -> VirtAddr {
	VirtAddr::new(0x1_0000_0000)
}
