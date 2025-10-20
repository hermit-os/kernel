pub(crate) mod allocator;
pub(crate) mod device_alloc;
pub(crate) mod physicalmem;
pub(crate) mod virtualmem;

use core::mem;
use core::ops::Range;

use align_address::Align;
use hermit_sync::Lazy;
pub use memory_addresses::{PhysAddr, VirtAddr};

use self::allocator::LockedAllocator;
#[cfg(any(target_arch = "x86_64", target_arch = "riscv64"))]
use crate::arch::mm::paging::HugePageSize;
pub use crate::arch::mm::paging::virtual_to_physical;
use crate::arch::mm::paging::{BasePageSize, LargePageSize, PageSize};
use crate::{arch, env};

#[cfg(all(target_os = "none", not(feature = "balloon")))]
#[global_allocator]
pub(crate) static ALLOCATOR: LockedAllocator = LockedAllocator::new();

#[cfg(all(target_os = "none", feature = "balloon"))]
#[global_allocator]
pub(crate) static ALLOCATOR: LockedAllocator = {
	// SAFETY: We are constructing this `LockedAllocator` to be Hermit's global
	//         allocator.
	unsafe { LockedAllocator::new() }
};

/// Physical and virtual address range of the 2 MiB pages that map the kernel.
static KERNEL_ADDR_RANGE: Lazy<Range<VirtAddr>> = Lazy::new(|| {
	if cfg!(target_os = "none") {
		// Calculate the start and end addresses of the 2 MiB page(s) that map the kernel.
		env::get_base_address().align_down(LargePageSize::SIZE)
			..(env::get_base_address() + env::get_image_size()).align_up(LargePageSize::SIZE)
	} else {
		VirtAddr::zero()..VirtAddr::zero()
	}
});

pub(crate) fn kernel_start_address() -> VirtAddr {
	KERNEL_ADDR_RANGE.start
}

pub(crate) fn kernel_end_address() -> VirtAddr {
	KERNEL_ADDR_RANGE.end
}

#[cfg(target_os = "none")]
pub(crate) fn init() {
	use crate::arch::mm::paging;

	Lazy::force(&KERNEL_ADDR_RANGE);

	arch::mm::init();
	arch::mm::init_page_tables();

	let total_mem = physicalmem::total_memory_size();
	let kernel_addr_range = KERNEL_ADDR_RANGE.clone();
	info!("Total memory size: {} MiB", total_mem >> 20);
	info!(
		"Kernel region: {:p}..{:p}",
		kernel_addr_range.start, kernel_addr_range.end
	);

	// we reserve physical memory for the required page tables
	// In worst case, we use page size of BasePageSize::SIZE
	let npages = total_mem / BasePageSize::SIZE as usize;
	let npage_3tables = npages / (BasePageSize::SIZE as usize / mem::align_of::<usize>()) + 1;
	let npage_2tables =
		npage_3tables / (BasePageSize::SIZE as usize / mem::align_of::<usize>()) + 1;
	let npage_1tables =
		npage_2tables / (BasePageSize::SIZE as usize / mem::align_of::<usize>()) + 1;
	let reserved_space = (npage_3tables + npage_2tables + npage_1tables)
		* BasePageSize::SIZE as usize
		+ 2 * LargePageSize::SIZE as usize;
	#[cfg(any(target_arch = "x86_64", target_arch = "riscv64"))]
	let has_1gib_pages = arch::processor::supports_1gib_pages();
	let has_2mib_pages = arch::processor::supports_2mib_pages();

	let min_mem = if env::is_uefi() {
		// On UEFI, the given memory is guaranteed free memory and the kernel is located before the given memory
		reserved_space
	} else {
		(kernel_addr_range.end.as_u64() - env::get_ram_address().as_u64() + reserved_space as u64)
			as usize
	};
	info!("Minimum memory size: {} MiB", min_mem >> 20);
	let avail_mem = total_mem
		.checked_sub(min_mem)
		.unwrap_or_else(|| panic!("Not enough memory available!"))
		.align_down(LargePageSize::SIZE as usize);

	let mut map_addr;
	let mut map_size;
	let heap_start_addr;

	#[cfg(feature = "common-os")]
	{
		info!("Using HermitOS as common OS!");

		// we reserve at least 75% of the memory for the user space
		let reserve: usize = (avail_mem * 75) / 100;
		// 64 MB is enough as kernel heap
		let reserve = core::cmp::min(reserve, 0x0400_0000);

		let virt_size: usize = reserve.align_down(LargePageSize::SIZE as usize);
		let virt_addr =
			self::virtualmem::allocate_aligned(virt_size, LargePageSize::SIZE as usize).unwrap();
		heap_start_addr = virt_addr;

		info!(
			"Heap: size {} MB, start address {:p}",
			virt_size >> 20,
			virt_addr
		);

		#[cfg(any(target_arch = "x86_64", target_arch = "riscv64"))]
		if has_1gib_pages && virt_size > HugePageSize::SIZE as usize {
			// Mount large pages to the next huge page boundary
			let npages = (virt_addr.align_up(HugePageSize::SIZE) - virt_addr) as usize
				/ LargePageSize::SIZE as usize;
			if let Err(n) = paging::map_heap::<LargePageSize>(virt_addr, npages) {
				map_addr = virt_addr + n as u64 * LargePageSize::SIZE;
				map_size = virt_size - (map_addr - virt_addr) as usize;
			} else {
				map_addr = virt_addr.align_up(HugePageSize::SIZE);
				map_size = virt_size - (map_addr - virt_addr) as usize;
			}
		} else {
			map_addr = virt_addr;
			map_size = virt_size;
		}

		#[cfg(not(any(target_arch = "x86_64", target_arch = "riscv64")))]
		{
			map_addr = virt_addr;
			map_size = virt_size;
		}
	}

	#[cfg(not(feature = "common-os"))]
	{
		// we reserve 10% of the memory for stack allocations
		let stack_reserve: usize = (avail_mem * 10) / 100;

		// At first, we map only a small part into the heap.
		// Afterwards, we already use the heap and map the rest into
		// the virtual address space.

		#[cfg(not(feature = "mmap"))]
		let virt_size: usize = (avail_mem - stack_reserve).align_down(LargePageSize::SIZE as usize);
		#[cfg(feature = "mmap")]
		let virt_size: usize = ((avail_mem * 75) / 100).align_down(LargePageSize::SIZE as usize);

		let virt_addr =
			crate::mm::virtualmem::allocate_aligned(virt_size, LargePageSize::SIZE as usize)
				.unwrap();
		heap_start_addr = virt_addr;

		info!(
			"Heap: size {} MB, start address {:p}",
			virt_size >> 20,
			virt_addr
		);

		#[cfg(any(target_arch = "x86_64", target_arch = "riscv64"))]
		if has_1gib_pages && virt_size > HugePageSize::SIZE as usize {
			// Mount large pages to the next huge page boundary
			let npages = (virt_addr.align_up(HugePageSize::SIZE) - virt_addr) / LargePageSize::SIZE;
			if let Err(n) = paging::map_heap::<LargePageSize>(virt_addr, npages as usize) {
				map_addr = virt_addr + n as u64 * LargePageSize::SIZE;
				map_size = virt_size - (map_addr - virt_addr) as usize;
			} else {
				map_addr = virt_addr.align_up(HugePageSize::SIZE);
				map_size = virt_size - (map_addr - virt_addr) as usize;
			}
		} else {
			map_addr = virt_addr;
			map_size = virt_size;
		}

		#[cfg(not(any(target_arch = "x86_64", target_arch = "riscv64")))]
		{
			map_addr = virt_addr;
			map_size = virt_size;
		}
	}

	#[cfg(any(target_arch = "x86_64", target_arch = "riscv64"))]
	if has_1gib_pages
		&& map_size > HugePageSize::SIZE as usize
		&& map_addr.is_aligned_to(HugePageSize::SIZE)
	{
		let size = map_size.align_down(HugePageSize::SIZE as usize);
		if let Err(num_pages) =
			paging::map_heap::<HugePageSize>(map_addr, size / HugePageSize::SIZE as usize)
		{
			map_size -= num_pages * HugePageSize::SIZE as usize;
			map_addr += num_pages as u64 * HugePageSize::SIZE;
		} else {
			map_size -= size;
			map_addr += size;
		}
	}

	if has_2mib_pages
		&& map_size > LargePageSize::SIZE as usize
		&& map_addr.is_aligned_to(LargePageSize::SIZE)
	{
		let size = map_size.align_down(LargePageSize::SIZE as usize);
		if let Err(num_pages) =
			paging::map_heap::<LargePageSize>(map_addr, size / LargePageSize::SIZE as usize)
		{
			map_size -= num_pages * LargePageSize::SIZE as usize;
			map_addr += num_pages as u64 * LargePageSize::SIZE;
		} else {
			map_size -= size;
			map_addr += size;
		}
	}

	if map_size > BasePageSize::SIZE as usize && map_addr.is_aligned_to(BasePageSize::SIZE) {
		let size = map_size.align_down(BasePageSize::SIZE as usize);
		if let Err(num_pages) =
			paging::map_heap::<BasePageSize>(map_addr, size / BasePageSize::SIZE as usize)
		{
			map_size -= num_pages * BasePageSize::SIZE as usize;
			map_addr += num_pages as u64 * BasePageSize::SIZE;
		} else {
			map_size -= size;
			map_addr += size;
		}
	}

	let heap_end_addr = map_addr;

	unsafe {
		ALLOCATOR.init(
			heap_start_addr.as_mut_ptr(),
			(heap_end_addr - heap_start_addr) as usize,
		);
	}

	info!("Heap is located at {heap_start_addr:p}..{heap_end_addr:p} ({map_size} Bytes unmapped)");
}

pub(crate) fn print_information() {
	self::physicalmem::print_information();
	self::virtualmem::print_information();
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

	let mut flags = PageTableEntryFlags::empty();
	flags.normal();
	if writable {
		flags.writable();
	}
	if no_execution {
		flags.execute_disable();
	}
	if no_cache {
		flags.device();
	}

	let virtual_address = self::virtualmem::allocate(size).unwrap();
	arch::mm::paging::map::<BasePageSize>(virtual_address, physical_address, count, flags);

	virtual_address
}

#[allow(dead_code)]
/// unmaps virtual address, without 'freeing' physical memory it is mapped to!
pub(crate) fn unmap(virtual_address: VirtAddr, size: usize) {
	let size = size.align_up(BasePageSize::SIZE as usize);

	if arch::mm::paging::virtual_to_physical(virtual_address).is_some() {
		arch::mm::paging::unmap::<BasePageSize>(
			virtual_address,
			size / BasePageSize::SIZE as usize,
		);
		self::virtualmem::deallocate(virtual_address, size);
	} else {
		panic!(
			"No page table entry for virtual address {:p}",
			virtual_address
		);
	}
}
