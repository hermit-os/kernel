pub(crate) mod paging;

#[cfg(feature = "common-os")]
use core::slice;

use memory_addresses::arch::x86_64::{PhysAddr, VirtAddr};
/// Copy the kernel stack pages of the current task to a new base address.
#[cfg(feature = "common-os")]
pub use paging::{clear_user_space, drop_user_space};
#[cfg(feature = "common-os")]
pub use x86_64::structures::paging::{PageSize, Size4KiB as BasePageSize};

#[cfg(feature = "common-os")]
use crate::arch::mm::paging::{PageTableEntryFlags, PageTableEntryFlagsExt};
use crate::mm::{FrameAlloc, PageAlloc, PageRangeAllocator};

#[cfg(feature = "common-os")]
pub fn create_new_root_page_table() -> usize {
	use free_list::PageLayout;
	use x86_64::registers::control::Cr3;

	use crate::mm::PageBox;

	let layout = PageLayout::from_size(BasePageSize::SIZE as usize).unwrap();
	let frame_range = FrameAlloc::allocate(layout).unwrap();
	let physaddr = PhysAddr::from(frame_range.start());

	let layout = PageLayout::from_size(2 * BasePageSize::SIZE as usize).unwrap();
	let page_range = PageBox::new(layout).unwrap();
	let virtaddr = VirtAddr::from(page_range.start());
	let mut flags = PageTableEntryFlags::empty();
	flags.normal().writable();

	let entry: u64 = unsafe {
		let (frame, _flags) = Cr3::read();
		paging::map::<BasePageSize>(virtaddr, frame.start_address().into(), 1, flags);
		let entry: &u64 = &*virtaddr.as_ptr();

		*entry
	};

	let slice_addr = virtaddr + BasePageSize::SIZE;
	paging::map::<BasePageSize>(slice_addr, physaddr, 1, flags);

	unsafe {
		let pml4 = slice::from_raw_parts_mut(slice_addr.as_mut_ptr(), 512);

		// clear PML4
		for elem in pml4.iter_mut() {
			*elem = 0;
		}

		// copy first element and the self reference
		pml4[0] = entry;
		// create self reference
		pml4[511] = physaddr.as_u64() + 0x3; // PG_PRESENT | PG_RW
	};

	paging::unmap::<BasePageSize>(virtaddr, 2);

	physaddr.as_usize()
}

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

// Allocate and initialize a private TLS region for a new user-space thread.
///
/// Maps a fresh user-accessible page range through the currently active
/// (shared) root page table, copies the per-process `TlsTemplate` into it,
/// sets up the trailing 8-byte TCB self-pointer, and returns the new
/// FS.Base value (the thread pointer) for the child thread.
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

	let mut flags = PageTableEntryFlags::empty();
	flags.normal().writable().user().execute_disable();
	paging::map::<BasePageSize>(
		virt_addr,
		phys_addr,
		total / BasePageSize::SIZE as usize,
		flags,
	);

	unsafe {
		// Copy the pristine PT_TLS image into the new block.
		virt_addr
			.as_mut_ptr::<u8>()
			.copy_from_nonoverlapping(template.init.as_ptr(), template.init.len());
		// Zero the rest of the TLS BSS area and the trailing TCB.
		virt_addr
			.as_mut_ptr::<u8>()
			.add(template.init.len())
			.write_bytes(0, total - template.init.len());
		// Variant II (x86_64): the thread pointer is at the start of the TCB
		// (right after the TLS data block) and stores its own address.
		let thread_ptr = virt_addr.as_u64() + template.size as u64;
		let tcb_ptr: *mut u64 = core::ptr::with_exposed_provenance_mut(thread_ptr as usize);
		tcb_ptr.write(thread_ptr);
		thread_ptr
	}
}

pub unsafe fn init() {
	paging::init();
	unsafe {
		FrameAlloc::init();
	}
	unsafe {
		paging::log_page_tables();
	}
	unsafe {
		PageAlloc::init();
	}

	#[cfg(feature = "common-os")]
	{
		use x86_64::registers::control::Cr3;

		let (frame, _flags) = Cr3::read();
		crate::scheduler::BOOT_ROOT_PAGE_TABLE
			.set(frame.start_address().as_u64().try_into().unwrap())
			.unwrap();
	}
}
