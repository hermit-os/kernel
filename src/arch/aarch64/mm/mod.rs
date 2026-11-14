pub mod paging;

#[cfg(all(feature = "common-os", feature = "fork"))]
pub use paging::{copy_current_root_page_table, copy_kernel_stack_to, prepare_mem_copy_on_write};
#[cfg(feature = "common-os")]
pub use paging::{create_new_root_page_table, drop_user_space};

/// Allocates a user-accessible TLS region and returns the value for `TPIDR_EL0`.
///
/// TLS Variant I: the thread pointer points to the 16-byte TCB, followed by
/// the TLS image.
#[cfg(feature = "common-os")]
pub fn allocate_thread_tls(template: &crate::scheduler::task::TlsTemplate) -> u64 {
	use align_address::Align;
	use free_list::PageLayout;
	use memory_addresses::arch::aarch64::{PhysAddr, VirtAddr};

	use crate::arch::aarch64::mm::paging::{self, BasePageSize, PageSize, PageTableEntryFlags};
	#[cfg(feature = "fork")]
	use crate::mm::frame_ref_inc;
	use crate::mm::page_range_alloc::PageRangeAllocator;
	use crate::mm::{FrameAlloc, PageAlloc};

	let tcb_size = 2 * size_of::<*mut ()>();
	let total = (tcb_size + template.size).align_up(BasePageSize::SIZE as usize);

	let virt_layout = PageLayout::from_size(total).unwrap();
	let virt_range = PageAlloc::allocate(virt_layout).unwrap();
	let virt_addr = VirtAddr::from(virt_range.start());

	let frame_layout = PageLayout::from_size(total).unwrap();
	let frame_range = FrameAlloc::allocate(frame_layout).unwrap();
	let phys_addr = PhysAddr::from(frame_range.start());
	#[cfg(feature = "fork")]
	for i in 0..total / BasePageSize::SIZE as usize {
		frame_ref_inc(phys_addr + i * BasePageSize::SIZE as usize);
	}

	let mut flags = PageTableEntryFlags::normal();
	flags.user();
	paging::map::<BasePageSize>(
		virt_addr,
		phys_addr,
		total / BasePageSize::SIZE as usize,
		flags,
	);

	unsafe {
		virt_addr.as_mut_ptr::<u8>().write_bytes(0, total);
		virt_addr
			.as_mut_ptr::<u8>()
			.add(tcb_size)
			.copy_from_nonoverlapping(template.init.as_ptr(), template.init.len());
	}

	virt_addr.as_u64()
}
