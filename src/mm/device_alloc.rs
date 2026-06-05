use core::alloc::{AllocError, Allocator, Layout};
use core::ptr::{self, NonNull};

use align_address::Align;
#[cfg(target_arch = "x86_64")]
use free_list::FreeList;
use free_list::{PageLayout, PageRange};
#[cfg(target_arch = "x86_64")]
use hermit_sync::InterruptTicketMutex;
use memory_addresses::{PhysAddr, VirtAddr};
#[cfg(target_arch = "x86_64")]
use x86_64::structures::paging::PhysFrame;

use crate::arch::mm::paging;
use crate::arch::mm::paging::{BasePageSize, HugePageSize, PageSize};
#[cfg(target_arch = "x86_64")]
use crate::arch::mm::paging::{LargePageSize, PageTableEntryFlags};
use crate::env;
use crate::mm::{FrameAlloc, PageRangeAllocator};

/// An [`Allocator`] for memory that is used to communicate with devices.
///
/// Allocations from this allocator always correspond to contiguous physical memory.
pub struct DeviceAlloc;

unsafe impl Allocator for DeviceAlloc {
	fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		assert!(layout.align() <= BasePageSize::SIZE as usize);
		let size = layout.size().align_up(BasePageSize::SIZE as usize);
		let frame_layout = PageLayout::from_size(size).unwrap();

		let frame_range = if const { DeviceAlloc.phys_offset().is_null() } {
			FrameAlloc::allocate(frame_layout)
		} else {
			cfg_select! {
				target_arch = "x86_64" => DeviceFreeList::allocate(frame_layout),
				_ => unreachable!()
			}
		}
		.map_err(|_| AllocError)?;

		let phys_addr = PhysAddr::from(frame_range.start());
		let ptr = self.ptr_from(phys_addr);
		let slice = ptr::slice_from_raw_parts_mut(ptr, size);
		Ok(NonNull::new(slice).unwrap())
	}

	unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
		assert!(layout.align() <= BasePageSize::SIZE as usize);
		let size = layout.size().align_up(BasePageSize::SIZE as usize);

		let phys_addr = self.phys_addr_from(ptr.as_ptr());
		let range = PageRange::from_start_len(phys_addr.as_usize(), size).unwrap();

		if const { DeviceAlloc.phys_offset().is_null() } {
			unsafe { FrameAlloc::deallocate(range) }
		} else {
			cfg_select! {
				target_arch = "x86_64" => unsafe { DeviceFreeList::deallocate(range) },
				_ => unreachable!()
			}
		};
	}
}

impl DeviceAlloc {
	/// Returns a pointer corresponding to `phys_addr`.
	#[inline]
	pub const fn ptr_from<T>(&self, phys_addr: PhysAddr) -> *mut T {
		let addr = phys_addr.as_usize() + const { Self.phys_offset().as_usize() };
		ptr::with_exposed_provenance_mut(addr)
	}

	/// Returns the physical address of `ptr`.
	///
	/// The address is only correct if `ptr` has been allocated by this allocator.
	#[inline]
	pub fn phys_addr_from<T: ?Sized>(&self, ptr: *mut T) -> PhysAddr {
		let addr =
			u64::try_from(ptr.expose_provenance()).unwrap() - const { Self.phys_offset().as_u64() };
		PhysAddr::new(addr)
	}

	/// Returns the physical address offset.
	///
	/// This device allocator expects the complete physical memory to be mapped device-readable at this offset.
	#[inline(always)]
	const fn phys_offset(&self) -> VirtAddr {
		cfg_select! {
			all(target_arch = "x86_64", careful) => VirtAddr::new(crate::mm::virtualmem::kernel_heap_end().as_u64().div_ceil(4)),
			_ => VirtAddr::zero(),
		}
	}

	pub fn init() {
		// Remove all mappings in the device allocator range
		if env::is_uefi() && DeviceAlloc.phys_offset() != VirtAddr::zero() {
			let start = DeviceAlloc.phys_offset();
			let count = DeviceAlloc.phys_offset().as_u64() / HugePageSize::SIZE;
			let count = usize::try_from(count).unwrap();
			paging::unmap::<HugePageSize>(start, count);
		}
	}
}

#[cfg(target_arch = "x86_64")]
static DEVICE_FREE_LIST: InterruptTicketMutex<FreeList<16>> =
	InterruptTicketMutex::new(FreeList::new());

#[cfg(target_arch = "x86_64")]
struct DeviceFreeList;

#[cfg(target_arch = "x86_64")]
type DeviceAllocIncrement = LargePageSize;

#[cfg(target_arch = "x86_64")]
impl DeviceFreeList {
	fn allocate(layout: PageLayout) -> Result<PageRange, AllocError> {
		let allocation = DEVICE_FREE_LIST.lock().allocate(layout);

		match allocation {
			Err(_) => {
				// Failed allocation: try to claim pages from the main FrameAllocator then retry
				let aligned_layout = PageLayout::from_size_align(
					layout.size().align_up(DeviceAllocIncrement::SIZE as usize),
					DeviceAllocIncrement::SIZE as usize,
				)
				.unwrap();

				let frames = FrameAlloc::allocate(aligned_layout)?;
				let start = x86_64::PhysAddr::new(u64::try_from(frames.start()).unwrap());
				let start = PhysFrame::<DeviceAllocIncrement>::from_start_address(start).unwrap();
				let end = x86_64::PhysAddr::new(u64::try_from(frames.end()).unwrap());
				let end = PhysFrame::<DeviceAllocIncrement>::from_start_address(end).unwrap();

				for frame in PhysFrame::range(start, end) {
					unsafe {
						Self::map_claim_frame(frame)?;
					}
				}

				// Retry allocation
				DEVICE_FREE_LIST
					.lock()
					.allocate(layout)
					.map_err(|_| AllocError)
			}
			Ok(r) => Ok(r),
		}
	}

	/// # Safety
	///
	/// See [PageRangeAllocator::deallocate]
	unsafe fn deallocate(range: PageRange) {
		// OPTIONAL: if we have too much memory in the list we may return it to the physical free list
		// In this case, we MUST unmap it/remap it as identity
		unsafe {
			// SAFETY: invariants match
			DEVICE_FREE_LIST.lock().deallocate(range).unwrap();
		}
	}

	/// Adds the given frame to the device free list.
	///
	/// The identity mapping for the frame will be removed and a new mapping will be inserted at
	/// the offset for devices.
	///
	/// The frame will then be added to the free list.
	///
	/// # Safety
	///
	/// The frame should have been clained from the free list
	unsafe fn map_claim_frame(frame: PhysFrame<DeviceAllocIncrement>) -> Result<(), AllocError> {
		let identity_mapping = VirtAddr::new(frame.start_address().as_u64());

		// Remove identity mapping
		paging::unmap::<DeviceAllocIncrement>(identity_mapping, 1);

		// Add mapping at the device offset
		let flags = PageTableEntryFlags::WRITABLE
			| PageTableEntryFlags::NO_EXECUTE
			| PageTableEntryFlags::WRITE_THROUGH;

		let phys_addr = frame.start_address().into();
		let virt_addr = VirtAddr::from_ptr(DeviceAlloc.ptr_from::<()>(phys_addr));
		paging::map::<DeviceAllocIncrement>(virt_addr, phys_addr, 1, flags);

		unsafe {
			DEVICE_FREE_LIST
				.lock()
				.deallocate(frame.into())
				.map_err(|_| AllocError)
		}
	}
}
