use core::alloc::{AllocError, Allocator, Layout};
use core::ptr::{self, NonNull};

use align_address::Align;

use crate::arch::mm::paging::{BasePageSize, PageSize};

/// An [`Allocator`] for memory that is used to communicate with devices.
///
/// Allocations from this allocator always correspond to contiguous physical memory.
pub struct DeviceAlloc;

unsafe impl Allocator for DeviceAlloc {
	fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		assert!(layout.align() <= BasePageSize::SIZE as usize);
		let size = layout.size().align_up(BasePageSize::SIZE as usize);
		let ptr = super::allocate(size, true).as_mut_ptr::<u8>();
		let slice = ptr::slice_from_raw_parts_mut(ptr, size);
		Ok(NonNull::new(slice).unwrap())
	}

	unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
		assert!(layout.align() <= BasePageSize::SIZE as usize);
		let size = layout.size().align_up(BasePageSize::SIZE as usize);
		let addr = ptr.as_ptr().expose_provenance().into();
		super::deallocate(addr, size);
	}
}
