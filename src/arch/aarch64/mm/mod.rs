pub(crate) mod paging;

#[cfg(feature = "common-os")]
pub use paging::{
	clear_user_space, copy_current_root_page_table, copy_kernel_stack_to, create_new_root_page_table,
	drop_user_space, get_current_root_page_table, prepare_mem_copy_on_write,
};

use crate::mm::{FrameAlloc, PageAlloc, PageRangeAllocator};

pub unsafe fn init() {
	unsafe {
		paging::init();
	}
	unsafe {
		FrameAlloc::init();
	}
	unsafe {
		PageAlloc::init();
	}

	#[cfg(feature = "common-os")]
	{
		use aarch64_cpu::registers::TTBR0_EL1;
		crate::scheduler::BOOT_ROOT_PAGE_TABLE
			.set(TTBR0_EL1.get_baddr() as usize)
			.unwrap();
	}
}
