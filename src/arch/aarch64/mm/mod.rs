pub mod paging;

#[cfg(feature = "common-os")]
pub use paging::{
	clear_user_space, copy_current_root_page_table, copy_kernel_stack_to, create_new_root_page_table,
	drop_user_space, get_current_root_page_table, prepare_mem_copy_on_write,
};
