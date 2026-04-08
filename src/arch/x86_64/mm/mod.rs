pub(crate) mod paging;

#[cfg(feature = "common-os")]
use memory_addresses::arch::x86_64::PhysAddr;
#[cfg(feature = "common-os")]
use x86_64::structures::paging::{PageSize, Size4KiB as BasePageSize};

/// Returns the physical address of the current task's root page table (PML4).
#[cfg(feature = "common-os")]
pub fn get_current_root_page_table() -> usize {
	use crate::arch::core_local::core_scheduler;
	core_scheduler().get_current_task().borrow().root_page_table
}

/// Copy the current task's PML4 into a new page table, sharing data pages (COW).
/// Returns the physical address of the new PML4.
#[cfg(feature = "common-os")]
pub fn copy_current_root_page_table() -> usize {
	use core::ptr;

	use free_list::PageLayout;
	use x86_64::structures::paging::PageTable;
	use x86_64::registers::control::Cr3;

	use crate::mm::{FrameAlloc, PageRangeAllocator};

	let layout = PageLayout::from_size(BasePageSize::SIZE as usize).unwrap();

	// Allocate a new PML4 frame
	let new_pml4_frame = FrameAlloc::allocate(layout).unwrap();
	let new_pml4_phys = PhysAddr::new(new_pml4_frame.start().try_into().unwrap());
	let new_pml4 = unsafe {
		&mut *ptr::with_exposed_provenance_mut::<PageTable>(
			new_pml4_phys.as_u64().try_into().unwrap(),
		)
	};

	// Access the current PML4 via Cr3 (identity-mapped: phys == virt)
	let (frame, _) = Cr3::read();
	let cur_pml4_phys = frame.start_address().as_u64();
	let cur_pml4 =
		unsafe { &*ptr::with_exposed_provenance::<PageTable>(cur_pml4_phys.try_into().unwrap()) };

	use x86_64::structures::paging::PageTableFlags;

	// Clear new PML4
	for entry in new_pml4.iter_mut() {
		entry.set_unused();
	}

	// Copy entries 1..511 (user-space) by deep-copying page table hierarchy.
	// Data pages themselves are shared (COW); only page-table pages are duplicated.
	for pml4_idx in 1..511usize {
		let cur_entry = &cur_pml4[pml4_idx];
		if !cur_entry.flags().contains(PageTableFlags::PRESENT) {
			continue;
		}

		let new_pdpt_frame = FrameAlloc::allocate(layout).unwrap();
		let new_pdpt_phys = PhysAddr::new(new_pdpt_frame.start().try_into().unwrap());
		new_pml4[pml4_idx].set_addr(new_pdpt_phys.into(), cur_entry.flags());

		let cur_pdpt = unsafe {
			&*ptr::with_exposed_provenance::<PageTable>(
				cur_entry.addr().as_u64().try_into().unwrap(),
			)
		};
		let new_pdpt = unsafe {
			&mut *ptr::with_exposed_provenance_mut::<PageTable>(
				new_pdpt_phys.as_u64().try_into().unwrap(),
			)
		};

		for pdpt_idx in 0..512usize {
			let cur_pdpt_entry = &cur_pdpt[pdpt_idx];
			if !cur_pdpt_entry.flags().contains(PageTableFlags::PRESENT) {
				new_pdpt[pdpt_idx].set_unused();
				continue;
			}

			let new_pd_frame = FrameAlloc::allocate(layout).unwrap();
			let new_pd_phys = PhysAddr::new(new_pd_frame.start().try_into().unwrap());
			new_pdpt[pdpt_idx].set_addr(new_pd_phys.into(), cur_pdpt_entry.flags());

			let cur_pd = unsafe {
				&*ptr::with_exposed_provenance::<PageTable>(
					cur_pdpt_entry.addr().as_u64().try_into().unwrap(),
				)
			};
			let new_pd = unsafe {
				&mut *ptr::with_exposed_provenance_mut::<PageTable>(
					new_pd_phys.as_u64().try_into().unwrap(),
				)
			};

			for pd_idx in 0..512usize {
				let cur_pd_entry = &cur_pd[pd_idx];
				if !cur_pd_entry.flags().contains(PageTableFlags::PRESENT) {
					new_pd[pd_idx].set_unused();
					continue;
				}

				let new_pt_frame = FrameAlloc::allocate(layout).unwrap();
				let new_pt_phys = PhysAddr::new(new_pt_frame.start().try_into().unwrap());
				new_pd[pd_idx].set_addr(new_pt_phys.into(), cur_pd_entry.flags());

				let cur_pt = unsafe {
					&*ptr::with_exposed_provenance::<PageTable>(
						cur_pd_entry.addr().as_u64().try_into().unwrap(),
					)
				};
				let new_pt = unsafe {
					&mut *ptr::with_exposed_provenance_mut::<PageTable>(
						new_pt_phys.as_u64().try_into().unwrap(),
					)
				};

				// Copy PT entries verbatim — data pages are shared (already COW-marked)
				*new_pt = cur_pt.clone();
			}
		}
	}

	// Entry 0: kernel low mapping — copy from current PML4
	unsafe {
		let src = &raw const cur_pml4[0];
		let dst = &raw mut new_pml4[0];
		*dst = (*src).clone();
	}

	// Entry 511: self-reference for the new PML4
	new_pml4[511].set_addr(
		new_pml4_phys.into(),
		PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
	);

	new_pml4_phys.as_usize()
}

/// Mark all writable user pages in the current page table as Copy-On-Write.
#[cfg(feature = "common-os")]
pub fn prepare_mem_copy_on_write() {
	paging::mark_user_pages_copy_on_write();
}

/// Copy the kernel stack pages of the current task to a new base address.
#[cfg(feature = "common-os")]
pub fn copy_kernel_stack_to(stack_address: usize) {
	paging::copy_kernel_stack_to(stack_address);
}
