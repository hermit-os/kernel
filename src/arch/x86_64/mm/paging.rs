use core::{fmt, ptr};

use free_list::PageLayout;
use x86_64::instructions::tlb;
use x86_64::registers::control::{Cr0, Cr0Flags, Cr2, Cr3};
#[cfg(feature = "common-os")]
use x86_64::registers::segmentation::SegmentSelector;
pub use x86_64::structures::idt::InterruptStackFrame as ExceptionStackFrame;
use x86_64::structures::idt::PageFaultErrorCode;
pub use x86_64::structures::paging::PageTableFlags as PageTableEntryFlags;
use x86_64::structures::paging::frame::PhysFrameRange;
use x86_64::structures::paging::mapper::{MapToError, MappedFrame, TranslateResult, UnmapError};
use x86_64::structures::paging::page::PageRange;
use x86_64::structures::paging::{
	FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PhysFrame, Size4KiB, Translate,
};

use crate::arch::x86_64::kernel::processor;
use crate::arch::x86_64::mm::{PhysAddr, VirtAddr};
use crate::mm::{FrameAlloc, PageRangeAllocator};
use crate::{env, scheduler};

unsafe impl FrameAllocator<Size4KiB> for FrameAlloc {
	fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
		let size = usize::try_from(Size4KiB::SIZE).unwrap();
		let layout = PageLayout::from_size(size).unwrap();

		let range = FrameAlloc::allocate(layout).ok()?;

		let phys_addr = PhysAddr::from(range.start());
		Some(PhysFrame::from_start_address(phys_addr.into()).unwrap())
	}
}

pub trait PageTableEntryFlagsExt {
	fn device(&mut self) -> &mut Self;

	fn normal(&mut self) -> &mut Self;

	#[cfg(feature = "acpi")]
	fn read_only(&mut self) -> &mut Self;

	fn writable(&mut self) -> &mut Self;

	fn execute_disable(&mut self) -> &mut Self;

	#[cfg(feature = "common-os")]
	fn execute_enable(&mut self) -> &mut Self;

	#[cfg(feature = "common-os")]
	fn user(&mut self) -> &mut Self;

	#[expect(dead_code)]
	#[cfg(feature = "common-os")]
	fn kernel(&mut self) -> &mut Self;

	/// Mark a page as Copy-On-Write: remove WRITABLE, remove DIRTY, set BIT_9 as COW marker.
	#[cfg(feature = "common-os")]
	fn copy_on_write(&mut self) -> &mut Self;
}

impl PageTableEntryFlagsExt for PageTableEntryFlags {
	fn device(&mut self) -> &mut Self {
		self.insert(PageTableEntryFlags::NO_CACHE);
		self
	}

	fn normal(&mut self) -> &mut Self {
		self.remove(PageTableEntryFlags::NO_CACHE);
		self
	}

	#[cfg(feature = "acpi")]
	fn read_only(&mut self) -> &mut Self {
		self.remove(PageTableEntryFlags::WRITABLE);
		self
	}

	fn writable(&mut self) -> &mut Self {
		self.insert(PageTableEntryFlags::WRITABLE);
		self
	}

	fn execute_disable(&mut self) -> &mut Self {
		self.insert(PageTableEntryFlags::NO_EXECUTE);
		self
	}

	#[cfg(feature = "common-os")]
	fn execute_enable(&mut self) -> &mut Self {
		self.remove(PageTableEntryFlags::NO_EXECUTE);
		self
	}

	#[cfg(feature = "common-os")]
	fn user(&mut self) -> &mut Self {
		self.insert(PageTableEntryFlags::USER_ACCESSIBLE);
		self
	}

	#[cfg(feature = "common-os")]
	fn kernel(&mut self) -> &mut Self {
		self.remove(PageTableEntryFlags::USER_ACCESSIBLE);
		self
	}

	#[cfg(feature = "common-os")]
	fn copy_on_write(&mut self) -> &mut Self {
		self.remove(PageTableEntryFlags::WRITABLE);
		self.remove(PageTableEntryFlags::DIRTY);
		self.insert(PageTableEntryFlags::BIT_9); // BIT_9 = COW marker
		self
	}
}

pub use x86_64::structures::paging::{
	PageSize, Size1GiB as HugePageSize, Size2MiB as LargePageSize, Size4KiB as BasePageSize,
};

/// Returns a mapping of the physical memory where physical address is equal to the virtual address (no offset)
pub unsafe fn identity_mapped_page_table() -> OffsetPageTable<'static> {
	let level_4_table_addr = Cr3::read().0.start_address().as_u64();
	let level_4_table_ptr =
		ptr::with_exposed_provenance_mut::<PageTable>(level_4_table_addr.try_into().unwrap());
	unsafe {
		let level_4_table = level_4_table_ptr.as_mut().unwrap();
		OffsetPageTable::new(level_4_table, x86_64::addr::VirtAddr::new(0x0))
	}
}

/// Translate a virtual memory address to a physical one.
pub fn virtual_to_physical(virtual_address: VirtAddr) -> Option<PhysAddr> {
	let addr = x86_64::VirtAddr::from(virtual_address);

	let translate_result = unsafe { identity_mapped_page_table() }.translate(addr);

	match translate_result {
		TranslateResult::NotMapped | TranslateResult::InvalidFrameAddress(_) => {
			trace!("Unable to determine the physical address of 0x{virtual_address:X}");
			None
		}
		TranslateResult::Mapped { frame, offset, .. } => {
			Some(PhysAddr::new((frame.start_address() + offset).as_u64()))
		}
	}
}

/// Maps a continuous range of pages.
///
/// # Arguments
///
/// * `physical_address` - First physical address to map these pages to
/// * `flags` - Flags from PageTableEntryFlags to set for the page table entry (e.g. WRITABLE or NO_EXECUTE).
///   The PRESENT flags is set automatically.
pub fn map<S>(
	virtual_address: VirtAddr,
	physical_address: PhysAddr,
	count: usize,
	flags: PageTableEntryFlags,
) where
	S: PageSize + fmt::Debug,
	for<'a> OffsetPageTable<'a>: Mapper<S>,
{
	let pages = {
		let start = Page::<S>::containing_address(virtual_address.into());
		let end = start + count as u64;
		Page::range(start, end)
	};

	let frames = {
		let start = PhysFrame::<S>::containing_address(physical_address.into());
		let end = start + count as u64;
		PhysFrame::range(start, end)
	};

	let flags = flags | PageTableEntryFlags::PRESENT;

	trace!("Mapping {pages:?} to {frames:?} with {flags:?}");

	unsafe fn map_pages<M, S>(
		mapper: &mut M,
		pages: PageRange<S>,
		frames: PhysFrameRange<S>,
		flags: PageTableEntryFlags,
	) -> bool
	where
		M: Mapper<S>,
		S: PageSize + fmt::Debug,
	{
		let mut unmapped = false;
		for (page, frame) in pages.zip(frames) {
			// TODO: Require explicit unmaps
			let unmap = mapper.unmap(page);
			if let Ok((_frame, flush)) = unmap {
				unmapped = true;
				flush.flush();
				debug!("Had to unmap page {page:?} before mapping.");
			}
			let map = unsafe { mapper.map_to(page, frame, flags, &mut FrameAlloc) };
			match map {
				Ok(mapper_flush) => mapper_flush.flush(),
				Err(err) => panic!("Could not map {page:?} to {frame:?}: {err:?}"),
			}
		}
		unmapped
	}

	let unmapped = unsafe { map_pages(&mut identity_mapped_page_table(), pages, frames, flags) };

	if unmapped {
		#[cfg(feature = "smp")]
		crate::arch::x86_64::kernel::apic::ipi_tlb_flush();
	}
}

/// Maps `count` pages at address `virt_addr`. If the allocation of a physical memory failed,
/// the number of successful mapped pages are returned as error value.
pub fn map_heap<S>(virt_addr: VirtAddr, count: usize) -> Result<(), usize>
where
	S: PageSize + fmt::Debug,
	for<'a> OffsetPageTable<'a>: Mapper<S>,
{
	let flags = {
		let mut flags = PageTableEntryFlags::empty();
		flags.normal().writable().execute_disable();
		flags
	};

	let virt_addrs = (0..count).map(|n| virt_addr + n as u64 * S::SIZE);

	for (map_counter, virt_addr) in virt_addrs.enumerate() {
		let layout = PageLayout::from_size_align(S::SIZE as usize, S::SIZE as usize).unwrap();
		let frame_range = FrameAlloc::allocate(layout).map_err(|_| map_counter)?;
		let phys_addr = PhysAddr::from(frame_range.start());
		map::<S>(virt_addr, phys_addr, 1, flags);
	}

	Ok(())
}

pub fn identity_map<S>(phys_addr: PhysAddr)
where
	S: PageSize + fmt::Debug,
	for<'a> OffsetPageTable<'a>: Mapper<S>,
{
	let frame = PhysFrame::<S>::from_start_address(phys_addr.into()).unwrap();
	let flags = PageTableEntryFlags::PRESENT
		| PageTableEntryFlags::WRITABLE
		| PageTableEntryFlags::NO_EXECUTE;
	let mapper_result =
		unsafe { identity_mapped_page_table().identity_map(frame, flags, &mut FrameAlloc) };

	match mapper_result {
		Ok(mapper_flush) => mapper_flush.flush(),
		Err(MapToError::PageAlreadyMapped(current_frame)) => assert_eq!(current_frame, frame),
		Err(MapToError::ParentEntryHugePage) => {
			let page_table = unsafe { identity_mapped_page_table() };
			let virt_addr = VirtAddr::new(frame.start_address().as_u64()).into();
			let phys_addr = frame.start_address();
			assert_eq!(page_table.translate_addr(virt_addr), Some(phys_addr));
		}
		Err(err) => panic!("could not identity-map {frame:?}: {err:?}"),
	}
}

pub fn unmap<S>(virtual_address: VirtAddr, count: usize)
where
	S: PageSize + fmt::Debug,
	for<'a> OffsetPageTable<'a>: Mapper<S>,
{
	trace!("Unmapping virtual address {virtual_address:p} ({count} pages)");

	let first_page = Page::<S>::containing_address(virtual_address.into());
	let last_page = first_page + count as u64;
	let range = Page::range(first_page, last_page);

	for page in range {
		let unmap_result = unsafe { identity_mapped_page_table() }.unmap(page);
		match unmap_result {
			Ok((_frame, flush)) => flush.flush(),
			// FIXME: Some sentinel pages around stacks are supposed to be unmapped.
			// We should handle this case there instead of here.
			Err(UnmapError::PageNotMapped) => {
				debug!("Tried to unmap {page:?}, which was not mapped.");
			}
			Err(err) => panic!("{err:?}"),
		}
	}
}

/// Walk user pages in the current PML4 and mark all writable user pages as Copy-On-Write.
/// This must be called before duplicating the page table for a fork.
#[cfg(feature = "common-os")]
pub fn mark_user_pages_copy_on_write() {
	// Since the kernel identity-maps all physical memory (phys addr == virt addr),
	// we can dereference physical addresses directly as page table pointers.
	let (frame, _) = Cr3::read();
	let pml4 = unsafe {
		&mut *ptr::with_exposed_provenance_mut::<PageTable>(frame.start_address().as_u64() as usize)
	};

	// Walk PML4 entries 1..511 (user-space entries).
	// Entry 0 is for the low kernel mapping; entry 511 is the self-reference.
	for pml4_idx in 1..511usize {
		let pml4_entry = &mut pml4[pml4_idx];
		if !pml4_entry.flags().contains(PageTableEntryFlags::PRESENT) {
			continue;
		}
		let pdpt = unsafe {
			&mut *ptr::with_exposed_provenance_mut::<PageTable>(pml4_entry.addr().as_u64() as usize)
		};
		for pdpt_idx in 0..512usize {
			let pdpt_entry = &mut pdpt[pdpt_idx];
			if !pdpt_entry.flags().contains(PageTableEntryFlags::PRESENT) {
				continue;
			}
			let pd = unsafe {
				&mut *ptr::with_exposed_provenance_mut::<PageTable>(
					pdpt_entry.addr().as_u64().try_into().unwrap(),
				)
			};
			for pd_idx in 0..512usize {
				let pd_entry = &mut pd[pd_idx];
				if !pd_entry.flags().contains(PageTableEntryFlags::PRESENT) {
					continue;
				}
				if pd_entry.flags().contains(PageTableEntryFlags::HUGE_PAGE) {
					// Skip huge pages (2 MiB) - not handled as COW here
					warn!("User space isn't able to use huge pages");
					continue;
				}
				let pt = unsafe {
					&mut *ptr::with_exposed_provenance_mut::<PageTable>(
						pd_entry.addr().as_u64().try_into().unwrap(),
					)
				};
				for pt_idx in 0..512usize {
					let pt_entry = &mut pt[pt_idx];
					if pt_entry.flags().contains(PageTableEntryFlags::PRESENT|PageTableEntryFlags::WRITABLE|PageTableEntryFlags::USER_ACCESSIBLE)
						&& !pt_entry.flags().contains(PageTableEntryFlags::BIT_9)
					{
						let new_flags = *pt_entry.flags().copy_on_write();
						pt_entry.set_addr(pt_entry.addr(), new_flags);
					}
				}
			}
		}
	}

	tlb::flush_all();
	#[cfg(feature = "smp")]
	crate::arch::x86_64::kernel::apic::ipi_tlb_flush();
}

#[cfg(feature = "common-os")]
pub fn drop_user_space(pml4_phys: usize) {
	debug!("Drop the user space at PML4 {pml4_phys:#x}");

	// Free a 4 KiB physical frame back to the frame allocator.
	fn free_frame(phys: usize) {
		let range = free_list::PageRange::new(phys, phys + free_list::PAGE_SIZE).unwrap();
		unsafe { FrameAlloc::deallocate(range) };
	}

	// Task::drop is usually called from another task's context (e.g. the
	// idle task running `cleanup_tasks`), so `Cr3::read()` would return the
	// page table of the cleaning task — not of the one being destroyed.
	// Walk the stored PML4 directly through the kernel identity map.
	let pml4 = unsafe { &mut *ptr::with_exposed_provenance_mut::<PageTable>(pml4_phys) };

	// Walk PML4 entries 1..511 (user-space entries).
	// Entry 0 is for the low kernel mapping; entry 511 is the self-reference.
	for pml4_idx in 1..511usize {
		let pml4_entry = &mut pml4[pml4_idx];
		if !pml4_entry.flags().contains(PageTableEntryFlags::PRESENT) {
			continue;
		}
		let pdpt_phys = pml4_entry.addr().as_u64() as usize;
		let pdpt = unsafe { &mut *ptr::with_exposed_provenance_mut::<PageTable>(pdpt_phys) };

		for pdpt_idx in 0..512usize {
			let pdpt_entry = &mut pdpt[pdpt_idx];
			if !pdpt_entry.flags().contains(PageTableEntryFlags::PRESENT) {
				continue;
			}
			let pd_phys = pdpt_entry.addr().as_u64() as usize;
			let pd = unsafe { &mut *ptr::with_exposed_provenance_mut::<PageTable>(pd_phys) };

			for pd_idx in 0..512usize {
				let pd_entry = &mut pd[pd_idx];
				if !pd_entry.flags().contains(PageTableEntryFlags::PRESENT) {
					continue;
				}
				if pd_entry.flags().contains(PageTableEntryFlags::HUGE_PAGE) {
					// Skip huge pages (2 MiB) - not handled as COW here
					warn!("User space isn't able to use huge pages");
					continue;
				}
				let pt_phys = pd_entry.addr().as_u64() as usize;
				let pt = unsafe { &mut *ptr::with_exposed_provenance_mut::<PageTable>(pt_phys) };

				for pt_idx in 0..512usize {
					let pt_entry = &mut pt[pt_idx];
					if pt_entry
						.flags()
						.contains(PageTableEntryFlags::PRESENT | PageTableEntryFlags::USER_ACCESSIBLE)
					{
						let phys_addr = PhysAddr::new(pt_entry.addr().as_u64());
						if crate::mm::frame_ref_dec(phys_addr) {
							free_frame(phys_addr.as_u64() as usize);
						}
						pt_entry.set_unused();
					}
				}

				// PT emptied → free the PT frame and clear the PD entry.
				free_frame(pt_phys);
				pd_entry.set_unused();
			}

			// PD emptied → free the PD frame and clear the PDPT entry.
			free_frame(pd_phys);
			pdpt_entry.set_unused();
		}

		// PDPT emptied → free the PDPT frame and clear the PML4 entry.
		free_frame(pdpt_phys);
		pml4_entry.set_unused();
	}

	// Finally free the PML4 itself. No TLB flush needed: the dropped page
	// table is not loaded on any core.
	free_frame(pml4_phys);
}

/// Copy the contents of the current task's kernel stack to a new virtual
/// base address. Used by fork: the child's `TaskStacks::new` has already
/// allocated and mapped fresh physical frames at `stack_address`, so we
/// simply `memcpy` the parent's stack pages into the child's mapping.
#[cfg(feature = "common-os")]
pub fn copy_kernel_stack_to(stack_address: usize) {
	use crate::arch::core_local::core_scheduler;

	let virt_addr = core_scheduler()
		.get_current_task()
		.borrow()
		.stacks
		.get_stack_virt_addr();
	let total_size = core_scheduler()
		.get_current_task()
		.borrow()
		.stacks
		.get_total_stack_size();

	let addr_diff = stack_address as u64 - virt_addr.as_u64();

	let page_table = unsafe { identity_mapped_page_table() };

	// The virtual layout has 4 guard pages (unmapped) interspersed; iterate
	// the full range including those so no mapped pages are missed.
	let full_virt_size = total_size as u64 + 4 * BasePageSize::SIZE;
	for i in (virt_addr.as_u64()..(virt_addr.as_u64() + full_virt_size))
		.step_by(BasePageSize::SIZE as usize)
	{
		let virt = x86_64::VirtAddr::new(i);
		match page_table.translate(virt) {
			TranslateResult::Mapped { .. } => {
				// Both src and dst are already mapped in the current page
				// table (dst via TaskStacks::new), so a plain memcpy suffices.
				let src = ptr::with_exposed_provenance::<u8>(i as usize);
				let dst = ptr::with_exposed_provenance_mut::<u8>((i + addr_diff) as usize);
				unsafe {
					dst.copy_from_nonoverlapping(src, BasePageSize::SIZE as usize);
				}
			}
			TranslateResult::NotMapped => {}
			TranslateResult::InvalidFrameAddress(_) => {
				error!("Unexpected translation result for stack page at {i:#X}");
				scheduler::abort();
			}
		}
	}

	tlb::flush_all();
}

#[cfg(not(feature = "common-os"))]
pub(crate) extern "x86-interrupt" fn page_fault_handler(
	stack_frame: ExceptionStackFrame,
	error_code: PageFaultErrorCode,
) {
	error!("Page fault (#PF)!");
	error!("page_fault_linear_address = {:p}", Cr2::read().unwrap());
	error!("error_code = {error_code:?}");
	error!("fs = {:#X}", processor::readfs());
	error!("gs = {:#X}", processor::readgs());
	error!("stack_frame = {stack_frame:#?}");
	scheduler::abort();
}

#[cfg(feature = "common-os")]
pub(crate) extern "x86-interrupt" fn page_fault_handler(
	mut stack_frame: ExceptionStackFrame,
	error_code: PageFaultErrorCode,
) {
	use core::arch::asm;

	use crate::arch::core_local::{core_scheduler, increment_irq_counter};
	let swapped_gs = unsafe {
		if stack_frame.as_mut().read().code_segment == SegmentSelector(0x08) {
			false
		} else {
			asm!("swapgs", options(nostack));
			true
		}
	};
	
	increment_irq_counter(14);

	let faulting_addr = Cr2::read().unwrap();
	let virtaddr = faulting_addr.align_down(BasePageSize::SIZE);

	debug!(
		"Task {} triggers a page fault at {:p}.",
		core_scheduler().get_current_task_id(),
		faulting_addr
	);

	// Handle Copy-On-Write faults: write fault on a read-only page with BIT_9 set.
	if error_code.contains(PageFaultErrorCode::CAUSED_BY_WRITE)
		&& !error_code.contains(PageFaultErrorCode::INSTRUCTION_FETCH)
	{
		let page_table = unsafe { identity_mapped_page_table() };
		if let TranslateResult::Mapped { flags, .. } = page_table.translate(virtaddr)
			&& flags.contains(PageTableEntryFlags::BIT_9)
			&& !flags.contains(PageTableEntryFlags::WRITABLE)
		{
			// COW fault: allocate a new page, copy old contents, map as writable.
			if let Some(src_phys) = virtual_to_physical(virtaddr.into()) {
				let mut new_flags = flags;
				new_flags.insert(PageTableEntryFlags::WRITABLE);
				new_flags.remove(PageTableEntryFlags::BIT_9);
				// Drop this task's COW reference
				let ref_is_zero = crate::mm::frame_ref_dec(src_phys);

				if ref_is_zero {
					crate::mm::frame_ref_inc(src_phys);
					map::<BasePageSize>(virtaddr.into(), src_phys, 1, new_flags);
				} else {
					let new_phys = crate::mm::copy_page(src_phys);
					crate::mm::frame_ref_inc(new_phys);
					map::<BasePageSize>(virtaddr.into(), new_phys, 1, new_flags);
				}

				if swapped_gs {
					unsafe {
						core::arch::asm!("swapgs", options(nostack));
					}
				}

				return;
			} else {
				error!("COW: failed to resolve physical address for {virtaddr:p}");
				scheduler::abort();
			}
		}
	}

	// Heap demand-paging: user-mode fault on an unmapped heap page.
	if error_code.contains(PageFaultErrorCode::USER_MODE) {
		let heap = core_scheduler().get_current_task().borrow().heap.clone();
		if heap.contains(virtaddr.into()) {
			let physaddr = {
				use free_list::PageLayout;

				use crate::mm::{FrameAlloc, PageRangeAllocator};
				let layout = PageLayout::from_size(BasePageSize::SIZE as usize).unwrap();
				let frame = FrameAlloc::allocate(layout).unwrap();
				PhysAddr::new(frame.start().try_into().unwrap())
			};
			let mut flags = PageTableEntryFlags::empty();
			flags.normal().user().writable().execute_disable();
			map::<BasePageSize>(virtaddr.into(), physaddr, 1, flags);
			crate::mm::frame_ref_inc(physaddr);
			unsafe {
				virtaddr.as_mut_ptr::<u8>().write_bytes(0, BasePageSize::SIZE as usize);
			}

			if swapped_gs {
				unsafe {
					asm!("swapgs", options(nostack));
				}
			}

			return;
		}
	}

	error!("Page fault (#PF)!");
	error!("page_fault_linear_address = {faulting_addr:p}");
	error!("error_code = {error_code:?}");
	error!("fs = {:#X}", processor::readfs());
	error!("gs = {:#X}", processor::readgs());
	error!("stack_frame = {stack_frame:#?}");
	scheduler::abort();
}

pub fn init() {
	unsafe {
		log_page_tables();
	}
	make_p4_writable();
}

fn make_p4_writable() {
	debug!("Making P4 table writable");

	if !env::is_uefi() {
		return;
	}

	let mut pt = unsafe { identity_mapped_page_table() };

	let p4_page = {
		let (p4_frame, _) = Cr3::read_raw();
		let p4_addr = x86_64::VirtAddr::new(p4_frame.start_address().as_u64());
		Page::<Size4KiB>::from_start_address(p4_addr).unwrap()
	};

	let TranslateResult::Mapped { frame, flags, .. } = pt.translate(p4_page.start_address()) else {
		unreachable!()
	};

	let make_writable = || unsafe {
		let flags = flags | PageTableEntryFlags::WRITABLE;
		match frame {
			MappedFrame::Size1GiB(_) => pt.set_flags_p3_entry(p4_page, flags).unwrap().ignore(),
			MappedFrame::Size2MiB(_) => pt.set_flags_p2_entry(p4_page, flags).unwrap().ignore(),
			MappedFrame::Size4KiB(_) => pt.update_flags(p4_page, flags).unwrap().ignore(),
		}
	};

	unsafe fn without_protect<F, R>(f: F) -> R
	where
		F: FnOnce() -> R,
	{
		let cr0 = Cr0::read();
		if cr0.contains(Cr0Flags::WRITE_PROTECT) {
			unsafe { Cr0::write(cr0 - Cr0Flags::WRITE_PROTECT) }
		}
		let ret = f();
		if cr0.contains(Cr0Flags::WRITE_PROTECT) {
			unsafe { Cr0::write(cr0) }
		}
		ret
	}

	unsafe { without_protect(make_writable) }
}

pub unsafe fn log_page_tables() {
	use log::Level;

	use self::mapped_page_range_display::OffsetPageTableExt;

	if !log_enabled!(Level::Trace) {
		return;
	}

	let page_table = unsafe { identity_mapped_page_table() };
	trace!("Page tables:\n{}", page_table.display());
}

pub mod mapped_page_range_display {
	use core::fmt::{self, Write};

	use x86_64::structures::paging::mapper::PageTableFrameMapping;
	use x86_64::structures::paging::{MappedPageTable, OffsetPageTable, PageSize};

	use super::mapped_page_table_iter::{
		self, MappedPageRangeInclusive, MappedPageRangeInclusiveItem,
		MappedPageTableRangeInclusiveIter,
	};
	use super::offset_page_table::PhysOffset;

	#[expect(dead_code)]
	pub trait MappedPageTableExt<P: PageTableFrameMapping + Clone> {
		fn display(&self) -> MappedPageTableDisplay<'_, &P>;
	}

	impl<P: PageTableFrameMapping + Clone> MappedPageTableExt<P> for MappedPageTable<'_, P> {
		fn display(&self) -> MappedPageTableDisplay<'_, &P> {
			MappedPageTableDisplay {
				inner: mapped_page_table_iter::mapped_page_table_range_iter(self),
			}
		}
	}

	pub trait OffsetPageTableExt {
		fn display(&self) -> MappedPageTableDisplay<'_, PhysOffset>;
	}

	impl OffsetPageTableExt for OffsetPageTable<'_> {
		fn display(&self) -> MappedPageTableDisplay<'_, PhysOffset> {
			MappedPageTableDisplay {
				inner: mapped_page_table_iter::offset_page_table_range_iter(self),
			}
		}
	}

	pub struct MappedPageTableDisplay<'a, P: PageTableFrameMapping + Clone> {
		inner: MappedPageTableRangeInclusiveIter<'a, P>,
	}

	impl<P: PageTableFrameMapping + Clone> fmt::Display for MappedPageTableDisplay<'_, P> {
		fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
			let mut has_fields = false;

			for mapped_page_range in self.inner.clone() {
				if has_fields {
					f.write_char('\n')?;
				}
				write!(f, "{}", mapped_page_range.display())?;

				has_fields = true;
			}

			Ok(())
		}
	}

	pub trait MappedPageRangeInclusiveItemExt {
		fn display(&self) -> MappedPageRangeInclusiveItemDisplay<'_>;
	}

	impl MappedPageRangeInclusiveItemExt for MappedPageRangeInclusiveItem {
		fn display(&self) -> MappedPageRangeInclusiveItemDisplay<'_> {
			MappedPageRangeInclusiveItemDisplay { inner: self }
		}
	}

	pub struct MappedPageRangeInclusiveItemDisplay<'a> {
		inner: &'a MappedPageRangeInclusiveItem,
	}

	impl fmt::Display for MappedPageRangeInclusiveItemDisplay<'_> {
		fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
			match self.inner {
				MappedPageRangeInclusiveItem::Size4KiB(range) => range.display().fmt(f),
				MappedPageRangeInclusiveItem::Size2MiB(range) => range.display().fmt(f),
				MappedPageRangeInclusiveItem::Size1GiB(range) => range.display().fmt(f),
			}
		}
	}

	pub trait MappedPageRangeInclusiveExt<S: PageSize> {
		fn display(&self) -> MappedPageRangeInclusiveDisplay<'_, S>;
	}

	impl<S: PageSize> MappedPageRangeInclusiveExt<S> for MappedPageRangeInclusive<S> {
		fn display(&self) -> MappedPageRangeInclusiveDisplay<'_, S> {
			MappedPageRangeInclusiveDisplay { inner: self }
		}
	}

	pub struct MappedPageRangeInclusiveDisplay<'a, S: PageSize> {
		inner: &'a MappedPageRangeInclusive<S>,
	}

	impl<S: PageSize> fmt::Display for MappedPageRangeInclusiveDisplay<'_, S> {
		fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
			let size = S::DEBUG_STR;
			let len = self.inner.page_range.len();
			let page_start = self.inner.page_range.start.start_address();
			let page_end = self.inner.page_range.end.start_address();
			let frame_start = self.inner.frame_range.start.start_address();
			let frame_end = self.inner.frame_range.end.start_address();
			let flags = self.inner.flags;
			let format_phys = if page_start.as_u64() == frame_start.as_u64() {
				assert_eq!(page_end.as_u64(), frame_end.as_u64());
				format_args!("{:>39}", "identity mapped")
			} else {
				format_args!("{frame_start:18p}..={frame_end:18p}")
			};
			write!(
				f,
				"size: {size}, len: {len:5}, virt: {page_start:18p}..={page_end:18p}, phys: {format_phys}, flags: {flags:?}"
			)
		}
	}
}

pub mod mapped_page_table_iter {
	//! TODO: try to upstream this to [`x86_64`].

	use core::fmt;
	use core::ops::{Add, AddAssign, Sub, SubAssign};

	use x86_64::structures::paging::frame::PhysFrameRangeInclusive;
	use x86_64::structures::paging::mapper::PageTableFrameMapping;
	use x86_64::structures::paging::page::{AddressNotAligned, PageRangeInclusive};
	use x86_64::structures::paging::{
		MappedPageTable, OffsetPageTable, Page, PageSize, PageTable, PageTableFlags,
		PageTableIndex, PhysFrame, Size1GiB, Size2MiB, Size4KiB,
	};

	use super::offset_page_table::PhysOffset;
	use super::walker::{PageTableWalkError, PageTableWalker};

	#[derive(Debug)]
	pub struct MappedPageRangeInclusive<S: PageSize> {
		pub page_range: PageRangeInclusive<S>,
		pub frame_range: PhysFrameRangeInclusive<S>,
		pub flags: PageTableFlags,
	}

	impl<S: PageSize> TryFrom<(MappedPage<S>, MappedPage<S>)> for MappedPageRangeInclusive<S> {
		type Error = TryFromMappedPageError;

		fn try_from((start, end): (MappedPage<S>, MappedPage<S>)) -> Result<Self, Self::Error> {
			if start.flags != end.flags {
				return Err(TryFromMappedPageError);
			}

			Ok(Self {
				page_range: PageRangeInclusive {
					start: start.page,
					end: end.page,
				},
				frame_range: PhysFrameRangeInclusive {
					start: start.frame,
					end: end.frame,
				},
				flags: start.flags,
			})
		}
	}

	#[derive(Debug)]
	pub enum MappedPageRangeInclusiveItem {
		Size4KiB(MappedPageRangeInclusive<Size4KiB>),
		Size2MiB(MappedPageRangeInclusive<Size2MiB>),
		Size1GiB(MappedPageRangeInclusive<Size1GiB>),
	}

	impl TryFrom<(MappedPageItem, MappedPageItem)> for MappedPageRangeInclusiveItem {
		type Error = TryFromMappedPageError;

		fn try_from((start, end): (MappedPageItem, MappedPageItem)) -> Result<Self, Self::Error> {
			match (start, end) {
				(MappedPageItem::Size4KiB(start), MappedPageItem::Size4KiB(end)) => {
					let range = MappedPageRangeInclusive::try_from((start, end))?;
					Ok(Self::Size4KiB(range))
				}
				(MappedPageItem::Size2MiB(start), MappedPageItem::Size2MiB(end)) => {
					let range = MappedPageRangeInclusive::try_from((start, end))?;
					Ok(Self::Size2MiB(range))
				}
				(MappedPageItem::Size1GiB(start), MappedPageItem::Size1GiB(end)) => {
					let range = MappedPageRangeInclusive::try_from((start, end))?;
					Ok(Self::Size1GiB(range))
				}
				(_, _) => Err(TryFromMappedPageError),
			}
		}
	}

	#[derive(PartialEq, Eq, Clone, Debug)]
	pub struct TryFromMappedPageError;

	impl fmt::Display for TryFromMappedPageError {
		fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
			f.write_str("provided mapped pages were not compatible")
		}
	}

	#[derive(Clone)]
	pub struct MappedPageTableRangeInclusiveIter<'a, P: PageTableFrameMapping> {
		inner: MappedPageTableIter<'a, P>,
		start: Option<MappedPageItem>,
		end: Option<MappedPageItem>,
	}

	pub fn mapped_page_table_range_iter<'a, P: PageTableFrameMapping>(
		page_table: &'a MappedPageTable<'a, P>,
	) -> MappedPageTableRangeInclusiveIter<'a, &'a P> {
		MappedPageTableRangeInclusiveIter {
			inner: mapped_page_table_iter(page_table),
			start: None,
			end: None,
		}
	}

	pub fn offset_page_table_range_iter<'a>(
		page_table: &'a OffsetPageTable<'a>,
	) -> MappedPageTableRangeInclusiveIter<'a, PhysOffset> {
		MappedPageTableRangeInclusiveIter {
			inner: offset_page_table_iter(page_table),
			start: None,
			end: None,
		}
	}

	impl<P: PageTableFrameMapping> Iterator for MappedPageTableRangeInclusiveIter<'_, P> {
		type Item = MappedPageRangeInclusiveItem;

		fn next(&mut self) -> Option<Self::Item> {
			if self.start.is_none() {
				self.start = self.inner.next();
				self.end = self.start;
			}

			let Some(start) = &mut self.start else {
				return None;
			};
			let end = self.end.as_mut().unwrap();

			for mapped_page in self.inner.by_ref() {
				if mapped_page == *end + 1 {
					*end = mapped_page;
					continue;
				}

				let range = MappedPageRangeInclusiveItem::try_from((*start, *end)).unwrap();
				*start = mapped_page;
				*end = mapped_page;
				return Some(range);
			}

			let range = MappedPageRangeInclusiveItem::try_from((*start, *end)).unwrap();
			self.start = None;
			self.end = None;
			Some(range)
		}
	}

	#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug)]
	pub struct MappedPage<S: PageSize> {
		pub page: Page<S>,
		pub frame: PhysFrame<S>,
		pub flags: PageTableFlags,
	}

	impl<S: PageSize> Add<u64> for MappedPage<S> {
		type Output = Self;

		fn add(self, rhs: u64) -> Self::Output {
			Self {
				page: self.page + rhs,
				frame: self.frame + rhs,
				flags: self.flags,
			}
		}
	}

	impl<S: PageSize> Sub<u64> for MappedPage<S> {
		type Output = Self;

		fn sub(self, rhs: u64) -> Self::Output {
			Self {
				page: self.page - rhs,
				frame: self.frame - rhs,
				flags: self.flags,
			}
		}
	}

	#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug)]
	pub enum MappedPageItem {
		Size4KiB(MappedPage<Size4KiB>),
		Size2MiB(MappedPage<Size2MiB>),
		Size1GiB(MappedPage<Size1GiB>),
	}

	impl Add<u64> for MappedPageItem {
		type Output = Self;

		fn add(self, rhs: u64) -> Self::Output {
			match self {
				Self::Size4KiB(mapped_page) => Self::Size4KiB(mapped_page + rhs),
				Self::Size2MiB(mapped_page) => Self::Size2MiB(mapped_page + rhs),
				Self::Size1GiB(mapped_page) => Self::Size1GiB(mapped_page + rhs),
			}
		}
	}

	impl AddAssign<u64> for MappedPageItem {
		fn add_assign(&mut self, rhs: u64) {
			*self = *self + rhs;
		}
	}

	impl Sub<u64> for MappedPageItem {
		type Output = Self;

		fn sub(self, rhs: u64) -> Self::Output {
			match self {
				Self::Size4KiB(mapped_page) => Self::Size4KiB(mapped_page - rhs),
				Self::Size2MiB(mapped_page) => Self::Size2MiB(mapped_page - rhs),
				Self::Size1GiB(mapped_page) => Self::Size1GiB(mapped_page - rhs),
			}
		}
	}

	impl SubAssign<u64> for MappedPageItem {
		fn sub_assign(&mut self, rhs: u64) {
			*self = *self - rhs;
		}
	}

	#[derive(Clone)]
	pub struct MappedPageTableIter<'a, P: PageTableFrameMapping> {
		page_table_walker: PageTableWalker<P>,
		level_4_table: &'a PageTable,
		p4_index: u16,
		p3_index: u16,
		p2_index: u16,
		p1_index: u16,
	}

	pub fn mapped_page_table_iter<'a, P: PageTableFrameMapping>(
		page_table: &'a MappedPageTable<'a, P>,
	) -> MappedPageTableIter<'a, &'a P> {
		MappedPageTableIter {
			page_table_walker: unsafe {
				PageTableWalker::new(page_table.page_table_frame_mapping())
			},
			level_4_table: page_table.level_4_table(),
			p4_index: 0,
			p3_index: 0,
			p2_index: 0,
			p1_index: 0,
		}
	}

	pub fn offset_page_table_iter<'a>(
		page_table: &'a OffsetPageTable<'a>,
	) -> MappedPageTableIter<'a, PhysOffset> {
		MappedPageTableIter {
			page_table_walker: unsafe {
				PageTableWalker::new(PhysOffset {
					offset: page_table.phys_offset(),
				})
			},
			level_4_table: page_table.level_4_table(),
			p4_index: 0,
			p3_index: 0,
			p2_index: 0,
			p1_index: 0,
		}
	}

	impl<P: PageTableFrameMapping> MappedPageTableIter<'_, P> {
		fn p4_index(&self) -> Option<PageTableIndex> {
			if self.p4_index >= 512 {
				return None;
			}

			Some(PageTableIndex::new(self.p4_index))
		}

		fn p3_index(&self) -> Option<PageTableIndex> {
			if self.p3_index >= 512 {
				return None;
			}

			Some(PageTableIndex::new(self.p3_index))
		}

		fn p2_index(&self) -> Option<PageTableIndex> {
			if self.p2_index >= 512 {
				return None;
			}

			Some(PageTableIndex::new(self.p2_index))
		}

		fn p1_index(&self) -> Option<PageTableIndex> {
			if self.p1_index >= 512 {
				return None;
			}

			Some(PageTableIndex::new(self.p1_index))
		}

		fn increment_p4_index(&mut self) -> Option<()> {
			if self.p4_index >= 511 {
				self.p4_index += 1;
				return None;
			}

			self.p4_index += 1;
			self.p3_index = 0;
			self.p2_index = 0;
			self.p1_index = 0;
			Some(())
		}

		fn increment_p3_index(&mut self) -> Option<()> {
			if self.p3_index == 511 {
				self.increment_p4_index()?;
				return None;
			}

			self.p3_index += 1;
			self.p2_index = 0;
			self.p1_index = 0;
			Some(())
		}

		fn increment_p2_index(&mut self) -> Option<()> {
			if self.p2_index == 511 {
				self.increment_p3_index()?;
				return None;
			}

			self.p2_index += 1;
			self.p1_index = 0;
			Some(())
		}

		fn increment_p1_index(&mut self) -> Option<()> {
			if self.p1_index == 511 {
				self.increment_p2_index()?;
				return None;
			}

			self.p1_index += 1;
			Some(())
		}

		fn next_forward(&mut self) -> Option<MappedPageItem> {
			let p4 = self.level_4_table;

			let p3 = loop {
				match self.page_table_walker.next_table(&p4[self.p4_index()?]) {
					Ok(page_table) => break page_table,
					Err(PageTableWalkError::NotMapped) => self.increment_p4_index()?,
					Err(PageTableWalkError::MappedToHugePage) => {
						panic!("level 4 entry has huge page bit set")
					}
				}
			};

			let p2 = loop {
				match self.page_table_walker.next_table(&p3[self.p3_index()?]) {
					Ok(page_table) => break page_table,
					Err(PageTableWalkError::NotMapped) => self.increment_p3_index()?,
					Err(PageTableWalkError::MappedToHugePage) => {
						let page =
							Page::from_page_table_indices_1gib(self.p4_index()?, self.p3_index()?);
						let entry = &p3[self.p3_index()?];
						let frame = PhysFrame::containing_address(entry.addr());
						let flags = entry.flags();
						let mapped_page =
							MappedPageItem::Size1GiB(MappedPage { page, frame, flags });

						self.increment_p3_index();
						return Some(mapped_page);
					}
				}
			};

			let p1 = loop {
				match self.page_table_walker.next_table(&p2[self.p2_index()?]) {
					Ok(page_table) => break page_table,
					Err(PageTableWalkError::NotMapped) => self.increment_p2_index()?,
					Err(PageTableWalkError::MappedToHugePage) => {
						let page = Page::from_page_table_indices_2mib(
							self.p4_index()?,
							self.p3_index()?,
							self.p2_index()?,
						);
						let entry = &p2[self.p2_index()?];
						let frame = PhysFrame::containing_address(entry.addr());
						let flags = entry.flags();
						let mapped_page =
							MappedPageItem::Size2MiB(MappedPage { page, frame, flags });

						self.increment_p2_index();
						return Some(mapped_page);
					}
				}
			};

			loop {
				let p1_entry = &p1[self.p1_index()?];

				if p1_entry.is_unused() {
					self.increment_p1_index()?;
					continue;
				}

				let frame = match PhysFrame::from_start_address(p1_entry.addr()) {
					Ok(frame) => frame,
					Err(AddressNotAligned) => {
						warn!("Invalid frame address: {:p}", p1_entry.addr());
						self.increment_p1_index()?;
						continue;
					}
				};

				let page = Page::from_page_table_indices(
					self.p4_index()?,
					self.p3_index()?,
					self.p2_index()?,
					self.p1_index()?,
				);
				let flags = p1_entry.flags();
				let mapped_page = MappedPageItem::Size4KiB(MappedPage { page, frame, flags });

				self.increment_p1_index();
				return Some(mapped_page);
			}
		}
	}

	impl<P: PageTableFrameMapping> Iterator for MappedPageTableIter<'_, P> {
		type Item = MappedPageItem;

		fn next(&mut self) -> Option<Self::Item> {
			self.next_forward().or_else(|| self.next_forward())
		}
	}
}

mod walker {
	//! Taken from [`x86_64`]

	use x86_64::structures::paging::PageTable;
	use x86_64::structures::paging::mapper::PageTableFrameMapping;
	use x86_64::structures::paging::page_table::{FrameError, PageTableEntry};

	#[derive(Clone, Debug)]
	pub(super) struct PageTableWalker<P: PageTableFrameMapping> {
		page_table_frame_mapping: P,
	}

	impl<P: PageTableFrameMapping> PageTableWalker<P> {
		#[inline]
		pub unsafe fn new(page_table_frame_mapping: P) -> Self {
			Self {
				page_table_frame_mapping,
			}
		}

		/// Internal helper function to get a reference to the page table of the next level.
		///
		/// Returns `PageTableWalkError::NotMapped` if the entry is unused. Returns
		/// `PageTableWalkError::MappedToHugePage` if the `HUGE_PAGE` flag is set
		/// in the passed entry.
		#[inline]
		pub(super) fn next_table<'b>(
			&self,
			entry: &'b PageTableEntry,
		) -> Result<&'b PageTable, PageTableWalkError> {
			let page_table_ptr = self
				.page_table_frame_mapping
				.frame_to_pointer(entry.frame()?);
			let page_table: &PageTable = unsafe { &*page_table_ptr };

			Ok(page_table)
		}
	}

	#[derive(Debug)]
	pub(super) enum PageTableWalkError {
		NotMapped,
		MappedToHugePage,
	}

	impl From<FrameError> for PageTableWalkError {
		#[inline]
		fn from(err: FrameError) -> Self {
			match err {
				FrameError::HugeFrame => PageTableWalkError::MappedToHugePage,
				FrameError::FrameNotPresent => PageTableWalkError::NotMapped,
			}
		}
	}
}

mod offset_page_table {
	//! Taken from [`x86_64`]

	use x86_64::VirtAddr;
	use x86_64::structures::paging::mapper::PageTableFrameMapping;
	use x86_64::structures::paging::{PageTable, PhysFrame};

	#[derive(Clone, Debug)]
	pub struct PhysOffset {
		pub offset: VirtAddr,
	}

	unsafe impl PageTableFrameMapping for PhysOffset {
		fn frame_to_pointer(&self, frame: PhysFrame) -> *mut PageTable {
			let virt = self.offset + frame.start_address().as_u64();
			virt.as_mut_ptr()
		}
	}
}
