use alloc::alloc::AllocError;

use free_list::{FreeList, PageLayout, PageRange};
use hermit_sync::InterruptTicketMutex;
use memory_addresses::VirtAddr;

static KERNEL_FREE_LIST: InterruptTicketMutex<FreeList<16>> =
	InterruptTicketMutex::new(FreeList::new());

/// Allocate virtual memory.
pub fn allocate_virtual(size: usize, align: usize) -> Result<VirtAddr, AllocError> {
	let page_range = KERNEL_FREE_LIST
		.lock()
		.allocate(PageLayout::from_size_align(size, align).unwrap())
		.map_err(|_| AllocError)?;
	Ok(VirtAddr::new(page_range.start() as u64))
}

/// Deallocate memory previously allocated with [allocate_virtual].
pub unsafe fn deallocate_virtual(addr: VirtAddr, size: usize) {
	unsafe {
		KERNEL_FREE_LIST
			.lock()
			.deallocate(PageRange::new(addr.as_u64() as usize, size).unwrap())
			.unwrap();
	};
}

pub fn print_virtual_free_list() {
	info!("virtual memory free list:\n{}", KERNEL_FREE_LIST.lock());
}

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
	cfg_if::cfg_if! {
		if #[cfg(target_arch = "aarch64")] {
			// maximum address, which can be supported by TTBR0
			VirtAddr::new(0xFFFF_FFFF_FFFF)
		} else if #[cfg(target_arch = "riscv64")] {
			// 256 GiB
			VirtAddr::new(0x0040_0000_0000 - 1)
		} else if #[cfg(target_arch = "x86_64")] {
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
	}
}
