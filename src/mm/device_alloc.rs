use core::alloc::{AllocError, Allocator, Layout};
use core::ptr::{self, NonNull};

use align_address::Align;

use crate::arch;
#[cfg(target_arch = "x86_64")]
use crate::arch::mm::paging::PageTableEntryFlagsExt;
use crate::arch::mm::paging::{BasePageSize, PageSize, PageTableEntryFlags};

/// An [`Allocator`] for memory that is used to communicate with devices.
///
/// Allocations from this allocator always correspond to contiguous physical memory.
pub struct DeviceAlloc;

unsafe impl Allocator for DeviceAlloc {
	fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		assert!(layout.align() <= BasePageSize::SIZE as usize);
		let size = layout.size().align_up(BasePageSize::SIZE as usize);

		let physical_address = super::physicalmem::allocate(size).unwrap();
		let virtual_address = super::virtualmem::allocate(size).unwrap();

		let count = size / BasePageSize::SIZE as usize;
		let mut flags = PageTableEntryFlags::empty();
		flags.normal().writable().execute_disable();
		arch::mm::paging::map::<BasePageSize>(virtual_address, physical_address, count, flags);

		let ptr = virtual_address.as_mut_ptr::<u8>();
		let slice = ptr::slice_from_raw_parts_mut(ptr, size);
		Ok(NonNull::new(slice).unwrap())
	}

	unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
		assert!(layout.align() <= BasePageSize::SIZE as usize);
		let size = layout.size().align_up(BasePageSize::SIZE as usize);
		let addr = ptr.as_ptr().expose_provenance().into();

		if let Some(phys_addr) = arch::mm::paging::virtual_to_physical(addr) {
			arch::mm::paging::unmap::<BasePageSize>(addr, size / BasePageSize::SIZE as usize);
			super::virtualmem::deallocate(addr, size);
			super::physicalmem::deallocate(phys_addr, size);
		} else {
			panic!("No page table entry for virtual address {addr:p}");
		}
	}
}
