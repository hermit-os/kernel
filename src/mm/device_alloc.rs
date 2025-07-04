use core::alloc::{AllocError, Allocator, Layout};
use core::ptr::{self, NonNull};

use align_address::Align;
use memory_addresses::PhysAddr;

use crate::arch::mm::paging::{BasePageSize, PageSize};

/// An [`Allocator`] for memory that is used to communicate with devices.
///
/// Allocations from this allocator always correspond to contiguous physical memory.
pub struct DeviceAlloc;

unsafe impl Allocator for DeviceAlloc {
	fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		assert!(layout.align() <= BasePageSize::SIZE as usize);
		let size = layout.size().align_up(BasePageSize::SIZE as usize);

		let phys_addr = super::physicalmem::allocate(size).unwrap();

		let ptr = ptr::with_exposed_provenance_mut(phys_addr.as_usize());
		let slice = ptr::slice_from_raw_parts_mut(ptr, size);
		Ok(NonNull::new(slice).unwrap())
	}

	unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
		assert!(layout.align() <= BasePageSize::SIZE as usize);
		let size = layout.size().align_up(BasePageSize::SIZE as usize);

		let phys_addr = self.phys_addr_from(ptr.as_ptr());

		super::physicalmem::deallocate(phys_addr, size);
	}
}

impl DeviceAlloc {
	/// Returns the physical address of `ptr`.
	///
	/// The address is only correct if `ptr` has been allocated by this allocator.
	pub fn phys_addr_from<T: ?Sized>(&self, ptr: *mut T) -> PhysAddr {
		let addr = u64::try_from(ptr.expose_provenance()).unwrap();
		PhysAddr::new(addr)
	}
}
