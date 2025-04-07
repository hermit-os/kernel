use free_list::PageRange;

use crate::arch::x86_64::mm::VirtAddr;
use crate::mm::virtualmem::KERNEL_FREE_LIST;

pub fn init() {
	let range = PageRange::new(
		kernel_heap_end().as_usize().div_ceil(2),
		kernel_heap_end().as_usize() + 1,
	)
	.unwrap();

	unsafe {
		KERNEL_FREE_LIST.lock().deallocate(range).unwrap();
	}
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
