//! Memory management.
//!
//! This is an overview of Hermit's memory layout:
//!
//! - `DeviceAlloc.device_offset` is 0 if `!cfg!(careful)`
//! - User space virtual memory is only used if `!cfg!(feature = "common-os")`
//! - On x86-64, PCI BARs, I/O APICs, and local APICs may be in `0xc0000000..0xffffffff`, which could be inside of `MEM`.
//!
//! ```text
//!                               Virtual address
//!                                    space
//!
//!                                 ...┌───┬──► 00000000
//!           Physical address   ...   │   │
//!                space      ...      │   │ Identity map
//!                        ...         │   │
//!    00000000 ◄──┬───┐...         ...├───┼──► mem_size
//!                │   │   ...   ...   │   │
//!     FrameAlloc │MEM│      ...      │   │ Unused
//!                │   │   ...   ...   │   │
//!    mem_size ◄──┼───┤...         ...├───┼──► DeviceAlloc.phys_offset
//!                │   │   ...         │   │
//!                │   │      ...      │   │ DeviceAlloc
//!                │   │         ...   │   │
//!          Empty │   │            ...├───┼──► DeviceAlloc.phys_offset + mem_size
//!                │   │               │   │
//!                │   │               │   │
//!                │   │               │   │ Unused
//!     Unknown ◄──┼───┤               │   │
//!                │   │               │   │
//!            PCI │   │               ├───┼──► kernel_virt_start
//!                │   │               │   │
//!     Unknown ◄──┼───┤               │   │ PageAlloc
//!                │   │               │   │
//!                │   │               ├───┼──► kernel_virt_end
//!                │   │               │   │
//!          Empty │   │               │   │
//!                │   │               │   │ User space
//!                │   │               │   │
//!                │   │               │   │
//! ```

pub(crate) mod device_alloc;
#[cfg(target_os = "none")]
mod lazy_heap;
mod mapped_page_box;
mod page_range_alloc;
mod physicalmem;
mod virtualmem;

use core::alloc::Layout;
use core::mem::MaybeUninit;

use align_address::Align;
use free_list::{PageLayout, PageRange};
pub use memory_addresses::{PhysAddr, VirtAddr};

pub use self::mapped_page_box::MappedPageBox;
pub use self::page_range_alloc::{PageRangeAllocator, PageRangeBox};
pub use self::physicalmem::{FrameAlloc, FrameBox};
pub use self::virtualmem::{PageAlloc, PageBox};
use crate::arch;
pub use crate::arch::mm::paging::virtual_to_physical;
use crate::arch::mm::paging::{BasePageSize, LargePageSize, PageSize};

#[cfg(target_os = "none")]
#[global_allocator]
pub(crate) static ALLOCATOR: lazy_heap::LazyHeap = lazy_heap::LazyHeap::new();

#[cfg(target_os = "none")]
pub(crate) fn claim_initial_heap() {
	#[repr(C, align(0x1000))]
	struct InitialHeap([MaybeUninit<u8>; 0x1000]);

	debug_assert_eq!(
		Layout::new::<InitialHeap>(),
		Layout::from_size_align(0x1000, 0x1000).unwrap()
	);

	static mut INITIAL_HEAP: InitialHeap = InitialHeap([MaybeUninit::uninit(); _]);

	let base = (&raw mut INITIAL_HEAP).cast::<u8>();
	let size = size_of::<InitialHeap>();
	unsafe {
		ALLOCATOR.lock().claim(base, size).unwrap();
	}
}

#[cfg(target_os = "none")]
pub(crate) fn init() {
	use crate::arch::mm::paging;

	#[cfg(not(target_arch = "riscv64"))]
	unsafe {
		paging::init();
	}
	let avail_frames = unsafe { FrameAlloc::init() };
	#[cfg(target_arch = "x86_64")]
	unsafe {
		paging::log_page_tables();
	}
	let _avail_pages = unsafe { PageAlloc::init() };
	#[cfg(target_arch = "riscv64")]
	unsafe {
		paging::enable_page_table();
	}

	info!("Total memory size: {} MiB", avail_frames >> 20);

	// we reserve physical memory for the required page tables
	// In worst case, we use page size of BasePageSize::SIZE
	let npages = avail_frames / BasePageSize::SIZE as usize;
	let npage_div = BasePageSize::SIZE as usize / align_of::<usize>();
	let npage_3tables = npages / npage_div + 1;
	let npage_2tables = npage_3tables / npage_div + 1;
	let npage_1tables = npage_2tables / npage_div + 1;
	let min_mem = (npage_3tables + npage_2tables + npage_1tables) * BasePageSize::SIZE as usize
		+ 2 * LargePageSize::SIZE as usize;
	info!("Minimum memory size: {} MiB", min_mem >> 20);
	let avail_mem = avail_frames
		.checked_sub(min_mem)
		.unwrap_or_else(|| panic!("Not enough memory available!"))
		.align_down(LargePageSize::SIZE as usize);

	#[cfg(feature = "common-os")]
	let virt_size: usize = {
		info!("Using Hermit as common OS!");

		// we reserve at least 75% of the memory for the user space
		let reserve: usize = (avail_mem * 75) / 100;
		// 64 MB is enough as kernel heap
		let reserve = core::cmp::min(reserve, 0x0400_0000);

		reserve.align_down(LargePageSize::SIZE as usize)
	};

	// The size of the physical memory is reserved for the heap to provide a
	// contiguous virtual range. Other memory mappings (stacks, mmap,...) use the
	// virtual memory past that.
	#[cfg(not(feature = "common-os"))]
	let virt_size: usize = avail_mem;

	let layout = PageLayout::from_size_align(virt_size, LargePageSize::SIZE as usize).unwrap();
	let page_range = PageAlloc::allocate(layout).unwrap();
	let virt_addr = VirtAddr::from(page_range.start());

	info!(
		"Heap: size {} MB, start address {:p}",
		virt_size >> 20,
		virt_addr
	);

	lazy_heap::init(virt_addr, virt_size);
}

pub(crate) fn print_information() {
	info!("{FrameAlloc}");
	info!("{PageAlloc}");
}

/// Maps a given physical address and size in virtual space and returns address.
#[cfg(feature = "pci")]
pub(crate) fn map(
	physical_address: PhysAddr,
	size: usize,
	writable: bool,
	no_execution: bool,
	no_cache: bool,
) -> VirtAddr {
	use crate::arch::mm::paging::PageTableEntryFlags;
	#[cfg(target_arch = "x86_64")]
	use crate::arch::mm::paging::PageTableEntryFlagsExt;

	let size = size.align_up(BasePageSize::SIZE as usize);
	let count = size / BasePageSize::SIZE as usize;

	let mut flags = if no_cache {
		PageTableEntryFlags::device()
	} else {
		PageTableEntryFlags::normal()
	};
	flags.set_writable(writable).set_execute(!no_execution);

	let layout = PageLayout::from_size(size).unwrap();
	let page_range = PageAlloc::allocate(layout).unwrap();
	let virtual_address = VirtAddr::from(page_range.start());
	arch::mm::paging::map::<BasePageSize>(virtual_address, physical_address, count, flags);

	virtual_address
}

#[allow(dead_code)]
/// Unmaps virtual address, without 'freeing' physical memory it is mapped to!
pub(crate) fn unmap(virtual_address: VirtAddr, size: usize) {
	let size = size.align_up(BasePageSize::SIZE as usize);

	if virtual_to_physical(virtual_address).is_some() {
		arch::mm::paging::unmap::<BasePageSize>(
			virtual_address,
			size / BasePageSize::SIZE as usize,
		);

		let range = PageRange::from_start_len(virtual_address.as_usize(), size).unwrap();
		unsafe {
			PageAlloc::deallocate(range);
		}
	} else {
		panic!("No page table entry for virtual address {virtual_address:p}");
	}
}
