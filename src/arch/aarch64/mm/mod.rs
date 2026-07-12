pub(crate) mod paging;

#[cfg(all(feature = "common-os", feature = "fork"))]
pub use paging::{copy_current_root_page_table, copy_kernel_stack_to, prepare_mem_copy_on_write};
#[cfg(feature = "common-os")]
pub use paging::{create_new_root_page_table, drop_user_space};

use crate::mm::{FrameAlloc, PageAlloc, PageRangeAllocator};

/// AArch64 sibling of the x86_64 [`allocate_thread_tls`].
///
/// Allocates a fresh user-accessible TLS region in the currently active
/// (shared) root page table and returns the value to install in
/// `TPIDR_EL0` for the new thread. AArch64 uses TLS Variant I: the
/// thread pointer points at the TCB which sits at the *start* of the
/// block, with the TLS image immediately following a 16-byte reserved
/// area (`tcb[0]`/`tcb[1]`). User code therefore reaches its TLS data
/// at `TPIDR_EL0 + 16 + offset`.
#[cfg(feature = "common-os")]
pub fn allocate_thread_tls(template: &crate::scheduler::task::TlsTemplate) -> u64 {
	use align_address::Align;
	use free_list::PageLayout;
	use memory_addresses::arch::aarch64::{PhysAddr, VirtAddr};

	use crate::arch::aarch64::mm::paging::{self, BasePageSize, PageSize, PageTableEntryFlags};
	#[cfg(feature = "fork")]
	use crate::mm::frame_ref_inc;

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

	let mut flags = PageTableEntryFlags::empty();
	flags.normal().writable().user().execute_disable();
	paging::map::<BasePageSize>(
		virt_addr,
		phys_addr,
		total / BasePageSize::SIZE as usize,
		flags,
	);

	unsafe {
		// Zero the whole region first (covers both the TCB and the TLS BSS area).
		virt_addr.as_mut_ptr::<u8>().write_bytes(0, total);
		// Copy the pristine PT_TLS init image into the slot that follows the TCB.
		virt_addr
			.as_mut_ptr::<u8>()
			.add(tcb_size)
			.copy_from_nonoverlapping(template.init.as_ptr(), template.init.len());
	}

	// Variant I: TPIDR_EL0 is the start of the TCB block.
	virt_addr.as_u64()
}

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
