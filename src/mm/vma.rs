use core::ops::Bound;

#[cfg(not(target_arch = "x86_64"))]
use memory_addresses::VirtAddr;
#[cfg(target_arch = "x86_64")]
use x86_64::VirtAddr;

use crate::core_local::core_scheduler;
use crate::errno::Errno;

/// A contiguous range of virtual addresses with uniform protection
/// and backing semantics, owned by one address space.
#[derive(Debug, Copy, Clone)]
pub struct VirtualMemoryArea {
	/// Inclusive start, page-aligned.
	pub start: VirtAddr,
	/// Exclusive end, page-aligned.
	pub end: VirtAddr,
	/// Protection bits the kernel must install when faulting in a page.
	pub prot: VirtualMemoryAreaProt,
	/// Description of the memory type
	#[allow(unused)]
	pub mem_type: MemoryType,
}

impl VirtualMemoryArea {
	pub fn new(
		start: VirtAddr,
		end: VirtAddr,
		prot: VirtualMemoryAreaProt,
		mem_type: MemoryType,
	) -> Self {
		Self {
			start,
			end,
			prot,
			mem_type,
		}
	}
}

#[derive(Debug, Copy, Clone)]
pub enum MemoryType {
	HEAP,
	STACK,
	CODE,
	TLS,
}

bitflags! {
	#[repr(transparent)]
	#[derive(Debug, Copy, Clone, Default)]
	pub struct VirtualMemoryAreaProt: u32 {
		/// Memory may not be accessed.
		const NONE = 0;
		/// Indicates that the memory region should be readable.
		const READ = 1 << 0;
		/// Indicates that the memory region should be writable.
		const WRITE = 1 << 1;
		/// Indicates that the memory region should be executable.
		const EXECUTE = 1 << 2;
	}
}

// Place the user heap inside the L0 slot that covers LOADER_START
// (`USER_L0_INDEX = LOADER_START >> 39 = 2`). On aarch64 only that
// slot is COW-marked at fork and deep-copied by
// `copy_current_root_page_table`; everything else is treated as a
// shared kernel mapping. Putting the heap outside L0[2] leaves it
// shared between parent and child forks, which manifests as the
// child overwriting the parent's heap on its first user-space write.
//
// LOADER_START is 0x0100_0000_0000; binaries currently take well
// under 256 GiB, so 0x0140_0000_0000 sits comfortably above any
// loaded image while still falling inside L0[2] (which ends at
// 0x0180_0000_0000). The address is canonical on x86_64 as well
// (bit 47 = 0).
const HEAP_START_ADDR: VirtAddr = VirtAddr::new(0x0140_0000_0000);

/// Creates a new virtual memory mapping of the `size` specified with
/// protection bits specified in `prot_flags`.
#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub extern "C" fn sys_mmap(
	size: usize,
	prot_flags: VirtualMemoryAreaProt,
	ret: &mut *mut u8,
) -> i32 {
	if *ret == core::ptr::null_mut() {
		let current_task = core_scheduler().get_current_task();
		let current_task_borrowed = current_task.borrow();
		let mut guard = current_task_borrowed.vmas.write();

		if let Some((_, vma)) = guard
			.range((Bound::Unbounded, Bound::Included(HEAP_START_ADDR)))
			.next_back()
		{
			if vma.end < HEAP_START_ADDR {
				let new_vma = VirtualMemoryArea::new(
					HEAP_START_ADDR,
					HEAP_START_ADDR + size as u64,
					prot_flags,
					MemoryType::HEAP,
				);
				guard.insert(HEAP_START_ADDR, new_vma);
				*ret = HEAP_START_ADDR.as_mut_ptr();

				return 0;
			} else {
				error!("Unable to create heap");

				return -i32::from(Errno::Nomem);
			}
		}
	} else {
		// Extend an existing VMA whose `end` equals the user-supplied
		// address. The caller passes the current upper bound of a region
		// it owns; the kernel grows that VMA by `size` bytes — provided
		// the next VMA (if any) starts far enough behind it.
		let addr = VirtAddr::from_ptr(*ret);
		let new_end = addr + size as u64;

		let current_task = core_scheduler().get_current_task();
		let current_task_borrowed = current_task.borrow();
		let mut guard = current_task_borrowed.vmas.write();

		// 1. Locate the predecessor: the VMA with largest start < addr.
		//    For an extend request its `end` must match `addr` exactly.
		let key = {
			let Some((key, vma)) = guard.range(..addr).next_back() else {
				return -i32::from(Errno::Inval);
			};
			if vma.end != addr {
				return -i32::from(Errno::Inval);
			}
			if !vma.prot.contains(prot_flags) {
				return -i32::from(Errno::Inval);
			}
			*key
		};

		// 2. The extension must not run into the next VMA's start.
		//    `VirtAddr::new(u64::MAX)` panics on x86_64 (non-canonical),
		//    so model "no successor" with `Option` instead of a sentinel.
		if let Some((&next_start, _)) = guard.range((Bound::Excluded(key), Bound::Unbounded)).next()
			&& new_end > next_start
		{
			return -i32::from(Errno::Nomem);
		}

		// 3. In-place extension.
		guard.get_mut(&key).unwrap().end = new_end;

		return 0;
	}

	return -i32::from(Errno::Inval);
}
