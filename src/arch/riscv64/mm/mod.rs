pub mod paging;

#[cfg(feature = "common-os")]
pub use paging::{create_new_root_page_table, drop_user_space};

use crate::mm::{FrameAlloc, PageAlloc, PageRangeAllocator};

pub unsafe fn init() {
	unsafe {
		FrameAlloc::init();
	}
	unsafe {
		PageAlloc::init();
	}
	unsafe {
		paging::enable_page_table();
	}

	#[cfg(feature = "common-os")]
	{
		paging::prepopulate_kernel_root();
		crate::scheduler::BOOT_ROOT_PAGE_TABLE
			.set(paging::kernel_root_page_table())
			.unwrap();
	}
}

/// Allocate a fresh per-thread TLS block from the current process's
/// per-process TLS template and return the thread-pointer value for the
/// new thread.
///
/// RISC-V uses TLS Variant I: `tp` points at the first byte of the TLS
/// data; the two-word TCB sits directly below it.
#[cfg(feature = "common-os")]
pub fn allocate_thread_tls(template: &crate::scheduler::task::TlsTemplate) -> u64 {
	use align_address::Align;
	use free_list::PageLayout;
	use memory_addresses::{PhysAddr, VirtAddr};

	use crate::arch::riscv64::mm::paging::{self, BasePageSize, PageSize, PageTableEntryFlags};

	let tcb_size = 2 * size_of::<*mut ()>();
	let total = (tcb_size + template.size).align_up(BasePageSize::SIZE as usize);

	let virt_layout = PageLayout::from_size(total).unwrap();
	let virt_range = PageAlloc::allocate(virt_layout).unwrap();
	let virt_addr = VirtAddr::from(virt_range.start());

	let frame_layout = PageLayout::from_size(total).unwrap();
	let frame_range = FrameAlloc::allocate(frame_layout).unwrap();
	let phys_addr = PhysAddr::from(frame_range.start());

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

	// Variant I: `tp` points at the TLS data that follows the TCB.
	(virt_addr + tcb_size as u64).as_u64()
}
