use core::alloc::AllocError;

use align_address::Align;
use free_list::{FreeList, PageLayout, PageRange};
use hermit_sync::InterruptTicketMutex;
use memory_addresses::VirtAddr;
use x86_64::PhysAddr;
use x86_64::structures::paging::{PageSize, PhysFrame, Size2MiB};

use crate::arch::mm::paging;
use crate::arch::mm::paging::{PageTableEntryFlags, PageTableEntryFlagsExt};
use crate::mm::device_alloc::DeviceAlloc;
use crate::mm::physicalmem::IdentityPageSize;
use crate::mm::{FrameAlloc, PageRangeAllocator};

static DEVICE_FREE_LIST: InterruptTicketMutex<FreeList<16>> =
	InterruptTicketMutex::new(FreeList::new());

pub(super) struct DeviceFreeList;

const _: () = {
	// Static assertions
	assert!(
		IdentityPageSize::SIZE == Size2MiB::SIZE,
		"identity pages must be precisely 2MB big"
	);
	assert!(
		super::device_alloc::ENABLE_PHYS_OFFSET,
		"physical offset must be enabled - are you sure compilation flags match between this module inclusion and the constant?"
	)
};

impl PageRangeAllocator for DeviceFreeList {
	unsafe fn init() {
		// Nothing to do
	}

	fn allocate(layout: PageLayout) -> Result<PageRange, AllocError> {
		let allocation = DEVICE_FREE_LIST.lock().allocate(layout);

		match allocation {
			Err(_) => {
				// Failed allocation: try to claim pages from the main FrameAllocator then retry
				let aligned_layout = PageLayout::from_size_align(
					layout.size().align_up(Size2MiB::SIZE as usize),
					Size2MiB::SIZE as usize,
				)
				.unwrap();

				let allocated_frames = FrameAlloc::allocate(aligned_layout)?;
				unsafe {
					Self::map_claim_frames(allocated_frames)?;
				}
				DEVICE_FREE_LIST
					.lock()
					.allocate(layout)
					.map_err(|_| AllocError)
			}
			Ok(r) => Ok(r),
		}
	}

	fn allocate_at(range: PageRange) -> Result<(), AllocError> {
		let allocation = DEVICE_FREE_LIST.lock().allocate_at(range);

		match allocation {
			Err(_) => {
				// Failed allocation: try to claim a page from the main FrameAllocator then retry
				let aligned_range = PageRange::new(
					range.start().align_down(Size2MiB::SIZE as usize),
					range.end().align_up(Size2MiB::SIZE as usize),
				)
				.unwrap();

				FrameAlloc::allocate_at(aligned_range)?;
				unsafe {
					Self::map_claim_frames(aligned_range)?;
				}
				DEVICE_FREE_LIST
					.lock()
					.allocate_at(range)
					.map_err(|_| AllocError)
			}
			Ok(r) => Ok(r),
		}
	}

	unsafe fn deallocate(range: PageRange) {
		DEVICE_FREE_LIST.lock().deallocate(range).unwrap();

		// OPTIONAL: if we have too much memory in the list we may return it to the physical free list
		// In this case, we MUST unmap it/remap it as identity
	}
}

impl DeviceFreeList {
	unsafe fn map_claim_frames(frames: PageRange) -> Result<(), AllocError> {
		assert!(
			frames.start().is_aligned_to(Size2MiB::SIZE as usize),
			"invalid range start alignment"
		);
		assert!(
			frames.end().is_aligned_to(Size2MiB::SIZE as usize),
			"invalid range end alignment"
		);

		let frames = (frames.start()..frames.end())
			.step_by(Size2MiB::SIZE as usize)
			.map(|start| PhysFrame::from_start_address(PhysAddr::new(start as u64)).unwrap());

		for frame in frames {
			Self::map_claim_frame(frame)?;
		}

		Ok(())
	}

	/// Claims the given frame into the device free list.
	///
	/// The identity mapping for the frame will be removed and a new mapping will be inserted at
	/// the offset for devices.
	///
	/// The frame will then be added to the free list.
	unsafe fn map_claim_frame(frame: PhysFrame<Size2MiB>) -> Result<(), AllocError> {
		// 1. Remove page table entry in identity mapped table for this page
		paging::unmap::<IdentityPageSize>(
			VirtAddr::new(frame.start_address().as_u64()),
			(Size2MiB::SIZE / IdentityPageSize::SIZE) as usize,
		);

		// 2. Add an entry at the device offset
		let flags = {
			let mut flags = PageTableEntryFlags::empty();
			flags.normal().writable().execute_disable().device();
			flags
		};

		let phys_addr = frame.start_address().into();
		let virt_addr = VirtAddr::from_ptr(DeviceAlloc.ptr_from::<()>(phys_addr));
		paging::map::<Size2MiB>(virt_addr, phys_addr, 1, flags);

		unsafe {
			DEVICE_FREE_LIST
				.lock()
				.deallocate(frame.into())
				.map_err(|_| AllocError)
		}
	}
}
