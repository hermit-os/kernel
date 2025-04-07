use align_address::Align;
use memory_addresses::VirtAddr;

use crate::arch;
#[cfg(target_arch = "x86_64")]
use crate::arch::mm::paging::PageTableEntryFlagsExt;
use crate::arch::mm::paging::{BasePageSize, PageSize, PageTableEntryFlags};

bitflags! {
	#[repr(transparent)]
	#[derive(Debug, Copy, Clone, Default)]
	pub struct MemoryProtection: u32 {
		/// Pages may not be accessed.
		const None = 0;
		/// Indicates that the memory region should be readable.
		const Read = 1 << 0;
		/// Indicates that the memory region should be writable.
		const Write = 1 << 1;
		/// Indicates that the memory region should be executable.
		const Exec = 1 << 2;
	}
}

/// Creates a new virtual memory mapping of the `size` specified with
/// protection bits specified in `prot_flags`.
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_mmap(size: usize, prot_flags: MemoryProtection, ret: &mut *mut u8) -> i32 {
	let size = size.align_up(BasePageSize::SIZE as usize);
	let virtual_address = arch::mm::virtualmem::allocate(size).unwrap();
	if prot_flags.is_empty() {
		*ret = virtual_address.as_mut_ptr();
		return 0;
	}
	let physical_address = crate::mm::physicalmem::allocate(size).unwrap();

	let count = size / BasePageSize::SIZE as usize;
	let mut flags = PageTableEntryFlags::empty();
	flags.normal().writable();
	if prot_flags.contains(MemoryProtection::Write) {
		flags.writable();
	}
	if !prot_flags.contains(MemoryProtection::Exec) {
		flags.execute_disable();
	}

	arch::mm::paging::map::<BasePageSize>(virtual_address, physical_address, count, flags);

	*ret = virtual_address.as_mut_ptr();

	0
}

/// Unmaps memory at the specified `ptr` for `size` bytes.
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_munmap(ptr: *mut u8, size: usize) -> i32 {
	let virtual_address = VirtAddr::from_ptr(ptr);
	let size = size.align_up(BasePageSize::SIZE as usize);

	if let Some(phys_addr) = arch::mm::paging::virtual_to_physical(virtual_address) {
		arch::mm::paging::unmap::<BasePageSize>(
			virtual_address,
			size / BasePageSize::SIZE as usize,
		);
		crate::mm::physicalmem::deallocate(phys_addr, size);
	}

	arch::mm::virtualmem::deallocate(virtual_address, size);

	0
}

/// Configures the protections associated with a region of virtual memory
/// starting at `ptr` and going to `size`.
///
/// Returns 0 on success and an error code on failure.
#[hermit_macro::system]
#[unsafe(no_mangle)]
pub extern "C" fn sys_mprotect(ptr: *mut u8, size: usize, prot_flags: MemoryProtection) -> i32 {
	let count = size / BasePageSize::SIZE as usize;
	let mut flags = PageTableEntryFlags::empty();
	flags.normal().writable();
	if prot_flags.contains(MemoryProtection::Write) {
		flags.writable();
	}
	if !prot_flags.contains(MemoryProtection::Exec) {
		flags.execute_disable();
	}

	let virtual_address = VirtAddr::from_ptr(ptr);

	if let Some(physical_address) = arch::mm::paging::virtual_to_physical(virtual_address) {
		arch::mm::paging::map::<BasePageSize>(virtual_address, physical_address, count, flags);
		0
	} else {
		let physical_address = crate::mm::physicalmem::allocate(size).unwrap();
		arch::mm::paging::map::<BasePageSize>(virtual_address, physical_address, count, flags);
		0
	}
}
