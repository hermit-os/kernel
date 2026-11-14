use core::ops::Bound;

#[cfg(not(target_arch = "x86_64"))]
use memory_addresses::VirtAddr;
#[cfg(target_arch = "x86_64")]
use x86_64::VirtAddr;

use crate::arch::kernel::core_local::core_scheduler;
use crate::errno::Errno;

/// A contiguous range of virtual addresses with uniform protection.
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

#[allow(clippy::upper_case_acronyms)]
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

// The user heap must be in the same L0 slot as the user binary (below
// 0x0180_0000_0000): on aarch64, only this slot is copied on fork.
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
	if (*ret).is_null() {
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
		// Extend the VMA that ends at the given address by `size` bytes.
		let addr = VirtAddr::from_ptr(*ret);
		let new_end = addr + size as u64;

		let current_task = core_scheduler().get_current_task();
		let current_task_borrowed = current_task.borrow();
		let mut guard = current_task_borrowed.vmas.write();

		// The predecessor's `end` must match `addr` exactly.
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

		// The extension must not run into the next VMA.
		if let Some((&next_start, _)) = guard.range((Bound::Excluded(key), Bound::Unbounded)).next()
			&& new_end > next_start
		{
			return -i32::from(Errno::Nomem);
		}

		guard.get_mut(&key).unwrap().end = new_end;

		return 0;
	}

	-i32::from(Errno::Inval)
}
