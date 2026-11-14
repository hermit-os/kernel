pub(crate) mod paging;

#[cfg(feature = "common-os")]
use memory_addresses::arch::x86_64::{PhysAddr, VirtAddr};
#[cfg(feature = "common-os")]
pub use paging::{clear_user_space, create_new_root_page_table, drop_user_space};
#[cfg(feature = "common-os")]
pub use x86_64::structures::paging::{PageSize, Size4KiB as BasePageSize};

#[cfg(feature = "common-os")]
use crate::arch::mm::paging::{PageTableEntryFlags, PageTableEntryFlagsExt};
#[cfg(feature = "common-os")]
use crate::mm::{FrameAlloc, PageAlloc, PageRangeAllocator};

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

/// Allocates a private TLS region for a new user-space thread and returns its FS base.
#[cfg(feature = "common-os")]
pub fn allocate_thread_tls(template: &crate::scheduler::task::TlsTemplate) -> u64 {
	use align_address::Align;
	use free_list::PageLayout;
	use x86_64::structures::paging::Size4KiB as BasePageSize;

	let tcb_size = size_of::<*mut ()>();
	let total = (template.size + tcb_size).align_up(BasePageSize::SIZE as usize);

	let virt_layout = PageLayout::from_size(total).unwrap();
	let virt_range = PageAlloc::allocate(virt_layout).unwrap();
	let virt_addr = VirtAddr::from(virt_range.start());

	let frame_layout = PageLayout::from_size(total).unwrap();
	let frame_range = FrameAlloc::allocate(frame_layout).unwrap();
	let phys_addr = PhysAddr::from(frame_range.start());

	let mut flags = PageTableEntryFlags::normal();
	flags.user();
	paging::map::<BasePageSize>(
		virt_addr,
		phys_addr,
		total / BasePageSize::SIZE as usize,
		flags,
	);

	unsafe {
		virt_addr
			.as_mut_ptr::<u8>()
			.copy_from_nonoverlapping(template.init.as_ptr(), template.init.len());
		virt_addr
			.as_mut_ptr::<u8>()
			.add(template.init.len())
			.write_bytes(0, total - template.init.len());
		// TLS Variant II: the thread pointer follows the TLS data and stores its own address.
		let thread_ptr = virt_addr.as_u64() + template.size as u64;
		let tcb_ptr: *mut u64 = core::ptr::with_exposed_provenance_mut(thread_ptr as usize);
		tcb_ptr.write(thread_ptr);
		thread_ptr
	}
}
