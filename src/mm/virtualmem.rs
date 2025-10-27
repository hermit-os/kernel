use core::alloc::AllocError;
use core::fmt;

use free_list::{FreeList, PageLayout, PageRange};
use hermit_sync::InterruptTicketMutex;
use memory_addresses::VirtAddr;

use crate::mm::{PageRangeAllocator, PageRangeBox};

static KERNEL_FREE_LIST: InterruptTicketMutex<FreeList<16>> =
	InterruptTicketMutex::new(FreeList::new());

pub struct PageAlloc;

impl PageRangeAllocator for PageAlloc {
	unsafe fn init() {
		unsafe {
			init();
		}
	}

	fn allocate(layout: PageLayout) -> Result<PageRange, AllocError> {
		KERNEL_FREE_LIST
			.lock()
			.allocate(layout)
			.map_err(|_| AllocError)
	}

	fn allocate_at(range: PageRange) -> Result<(), AllocError> {
		KERNEL_FREE_LIST
			.lock()
			.allocate_at(range)
			.map_err(|_| AllocError)
	}

	unsafe fn deallocate(range: PageRange) {
		unsafe {
			KERNEL_FREE_LIST.lock().deallocate(range).unwrap();
		}
	}
}

impl fmt::Display for PageAlloc {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let free_list = KERNEL_FREE_LIST.lock();
		write!(f, "PageAlloc free list:\n{free_list}")
	}
}

pub type PageBox = PageRangeBox<PageAlloc>;

unsafe fn init() {
	let range = PageRange::new(
		kernel_heap_end().as_usize().div_ceil(2),
		kernel_heap_end().as_usize() + 1,
	)
	.unwrap();

	unsafe {
		PageAlloc::deallocate(range);
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
