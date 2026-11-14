pub(crate) mod paging;

#[cfg(feature = "common-os")]
pub use paging::{clear_user_space, create_new_root_page_table, drop_user_space};
#[cfg(feature = "common-os")]
pub use x86_64::structures::paging::{PageSize, Size4KiB as BasePageSize};

/// Returns the physical address of the current task's root page table (PML4).
#[allow(dead_code)]
#[cfg(feature = "common-os")]
pub fn get_current_root_page_table() -> usize {
	use crate::arch::kernel::core_local::core_scheduler;
	core_scheduler()
		.get_current_task()
		.borrow()
		.root_page_table
		.as_usize()
}
