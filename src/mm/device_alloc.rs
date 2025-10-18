use core::alloc::{AllocError, Allocator, Layout};
use core::ptr::{self, NonNull};

use align_address::Align;
use free_list::PageRange;
use memory_addresses::{PhysAddr, VirtAddr};

use crate::arch::mm::paging::{BasePageSize, PageSize};
use crate::mm::physicalmem::{allocate_physical, deallocate_physical};
use crate::mm::virtualmem;

/// An [`Allocator`] for memory that is used to communicate with devices.
///
/// Allocations from this allocator always correspond to contiguous physical memory.
pub struct DeviceAlloc;

unsafe impl Allocator for DeviceAlloc {
	fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		assert!(layout.align() <= BasePageSize::SIZE as usize);
		let size = layout.size().align_up(BasePageSize::SIZE as usize);
		let phys_addr = allocate_physical(size, free_list::PAGE_SIZE).map_err(|_| AllocError)?;
		let ptr = self.ptr_from(phys_addr);
		let slice = ptr::slice_from_raw_parts_mut(ptr, size);
		Ok(NonNull::new(slice).unwrap())
	}

	unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
		assert!(layout.align() <= BasePageSize::SIZE as usize);
		let size = layout.size().align_up(BasePageSize::SIZE as usize);

		let phys_addr = self.phys_addr_from(ptr.as_ptr());
		let range = PageRange::from_start_len(phys_addr.as_usize(), size).unwrap();

		unsafe {
			deallocate_physical(PhysAddr::new(range.start() as u64), range.len().get());
		}
	}
}

impl DeviceAlloc {
	/// Returns a pointer corresponding to `phys_addr`.
	#[inline]
	pub fn ptr_from<T>(&self, phys_addr: PhysAddr) -> *mut T {
		let addr = phys_addr.as_usize() + self.phys_offset().as_usize();
		ptr::with_exposed_provenance_mut(addr)
	}

	/// Returns the physical address of `ptr`.
	///
	/// The address is only correct if `ptr` has been allocated by this allocator.
	#[inline]
	pub fn phys_addr_from<T: ?Sized>(&self, ptr: *mut T) -> PhysAddr {
		let addr = u64::try_from(ptr.expose_provenance()).unwrap() - self.phys_offset().as_u64();
		PhysAddr::new(addr)
	}

	/// Returns the physical address offset.
	///
	/// This device allocator expects the complete physical memory to be mapped device-readable at this offset.
	#[inline]
	pub fn phys_offset(&self) -> VirtAddr {
		if cfg!(careful) {
			virtualmem::kernel_heap_end().as_u64().div_ceil(4).into()
		} else {
			0u64.into()
		}
	}
}
