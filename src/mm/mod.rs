pub mod allocator;
pub mod device_alloc;

use core::mem;
use core::ops::Range;

use align_address::Align;
use hermit_sync::Lazy;
#[cfg(feature = "newlib")]
use hermit_sync::OnceCell;

use self::allocator::LockedAllocator;
#[cfg(any(target_arch = "x86_64", target_arch = "riscv64"))]
use crate::arch::mm::paging::HugePageSize;
#[cfg(target_arch = "x86_64")]
use crate::arch::mm::paging::PageTableEntryFlagsExt;
use crate::arch::mm::paging::{BasePageSize, LargePageSize, PageSize, PageTableEntryFlags};
use crate::arch::mm::physicalmem::total_memory_size;
#[cfg(feature = "newlib")]
use crate::arch::mm::virtualmem::kernel_heap_end;
#[cfg(feature = "pci")]
use crate::arch::mm::PhysAddr;
use crate::arch::mm::VirtAddr;
use crate::{arch, env};

#[cfg(target_os = "none")]
#[global_allocator]
pub static ALLOCATOR: LockedAllocator = LockedAllocator::new();

/// Physical and virtual address range of the 2 MiB pages that map the kernel.
static KERNEL_ADDR_RANGE: Lazy<Range<VirtAddr>> = Lazy::new(|| {
	if cfg!(target_os = "none") {
		// Calculate the start and end addresses of the 2 MiB page(s) that map the kernel.
		env::get_base_address().align_down_to_large_page()
			..(env::get_base_address() + env::get_image_size()).align_up_to_large_page()
	} else {
		VirtAddr::zero()..VirtAddr::zero()
	}
});

#[cfg(feature = "newlib")]
/// User heap address range.
static HEAP_ADDR_RANGE: OnceCell<Range<VirtAddr>> = OnceCell::new();

pub(crate) fn kernel_start_address() -> VirtAddr {
	KERNEL_ADDR_RANGE.start
}

pub(crate) fn kernel_end_address() -> VirtAddr {
	KERNEL_ADDR_RANGE.end
}

#[cfg(feature = "newlib")]
pub(crate) fn task_heap_start() -> VirtAddr {
	HEAP_ADDR_RANGE.get().unwrap().start
}

#[cfg(feature = "newlib")]
pub(crate) fn task_heap_end() -> VirtAddr {
	HEAP_ADDR_RANGE.get().unwrap().end
}

#[cfg(target_os = "none")]
pub(crate) fn init() {
	use crate::arch::mm::paging;

	Lazy::force(&KERNEL_ADDR_RANGE);

	arch::mm::init();
	arch::mm::init_page_tables();

	info!("Total memory size: {} MB", total_memory_size() >> 20);
	info!(
		"Kernel region: [{:p} - {:p}]",
		kernel_start_address(),
		kernel_end_address()
	);

	// we reserve physical memory for the required page tables
	// In worst case, we use page size of BasePageSize::SIZE
	let npages = total_memory_size() / BasePageSize::SIZE as usize;
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

	//info!("reserved space {} KB", reserved_space >> 10);

	if total_memory_size()
		< kernel_end_address().as_usize() - env::get_ram_address().as_usize()
			+ reserved_space
			+ LargePageSize::SIZE as usize
	{
		panic!("No enough memory available!");
	}

	let mut map_addr: VirtAddr;
	let mut map_size: usize;

	let available_memory = (total_memory_size()
		- (kernel_end_address().as_usize() - env::get_ram_address().as_usize())
		- reserved_space)
		.align_down(LargePageSize::SIZE as usize);

	let heap_start_addr;

	#[cfg(all(feature = "newlib", not(feature = "common-os")))]
	{
		// we reserve 10% of the memory for stack allocations
		let stack_reserve: usize = (available_memory * 10) / 100;

		info!("An application with a C-based runtime is running on top of Hermit!");
		let kernel_heap_size = 10 * LargePageSize::SIZE as usize;

		unsafe {
			let start = {
				let physical_address = arch::mm::physicalmem::allocate(kernel_heap_size).unwrap();
				let virtual_address = arch::mm::virtualmem::allocate(kernel_heap_size).unwrap();

				let count = kernel_heap_size / BasePageSize::SIZE as usize;
				let mut flags = PageTableEntryFlags::empty();
				flags.normal().writable().execute_disable();
				arch::mm::paging::map::<BasePageSize>(
					virtual_address,
					physical_address,
					count,
					flags,
				);

				virtual_address
			};
			ALLOCATOR.init(start.as_mut_ptr(), kernel_heap_size);

			info!("Kernel heap starts at {:#x}", start);
		}

		info!("Kernel heap size: {} MB", kernel_heap_size >> 20);
		let user_heap_size =
			(available_memory - kernel_heap_size - stack_reserve - LargePageSize::SIZE as usize)
				.align_down(LargePageSize::SIZE as usize);
		info!("User-space heap size: {} MB", user_heap_size >> 20);

		map_addr = kernel_heap_end();
		map_size = user_heap_size;
		heap_start_addr = map_addr;
	}

	#[cfg(all(not(feature = "newlib"), feature = "common-os"))]
	{
		info!("Using HermitOS as common OS!");

		// we reserve at least 75% of the memory for the user space
		let reserve: usize = (available_memory * 75) / 100;
		// 64 MB is enough as kernel heap
		let reserve = core::cmp::min(reserve, 0x4000000);

		let virt_size: usize = reserve.align_down(LargePageSize::SIZE as usize);
		let virt_addr =
			arch::mm::virtualmem::allocate_aligned(virt_size, LargePageSize::SIZE as usize)
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
			let npages = (virt_addr.align_up_to_huge_page().as_usize() - virt_addr.as_usize())
				/ LargePageSize::SIZE as usize;
			if let Err(n) = paging::map_heap::<LargePageSize>(virt_addr, npages) {
				map_addr = virt_addr + n * LargePageSize::SIZE as usize;
				map_size = virt_size - (map_addr - virt_addr).as_usize();
			} else {
				map_addr = virt_addr.align_up_to_huge_page();
				map_size = virt_size - (map_addr - virt_addr).as_usize();
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

	#[cfg(all(not(feature = "newlib"), not(feature = "common-os")))]
	{
		// we reserve 10% of the memory for stack allocations
		let stack_reserve: usize = (available_memory * 10) / 100;

		info!("A pure Rust application is running on top of Hermit!");

		// At first, we map only a small part into the heap.
		// Afterwards, we already use the heap and map the rest into
		// the virtual address space.

		let virt_size: usize =
			(available_memory - stack_reserve).align_down(LargePageSize::SIZE as usize);
		let virt_addr =
			arch::mm::virtualmem::allocate_aligned(virt_size, LargePageSize::SIZE as usize)
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
			let npages = (virt_addr.align_up_to_huge_page().as_usize() - virt_addr.as_usize())
				/ LargePageSize::SIZE as usize;
			if let Err(n) = paging::map_heap::<LargePageSize>(virt_addr, npages) {
				map_addr = virt_addr + n * LargePageSize::SIZE as usize;
				map_size = virt_size - (map_addr - virt_addr).as_usize();
			} else {
				map_addr = virt_addr.align_up_to_huge_page();
				map_size = virt_size - (map_addr - virt_addr).as_usize();
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
		&& map_addr.is_aligned(HugePageSize::SIZE)
	{
		let size = map_size.align_down(HugePageSize::SIZE as usize);
		if let Err(num_pages) =
			paging::map_heap::<HugePageSize>(map_addr, size / HugePageSize::SIZE as usize)
		{
			map_size -= num_pages * HugePageSize::SIZE as usize;
			map_addr += num_pages * HugePageSize::SIZE as usize;
		} else {
			map_size -= size;
			map_addr += size;
		}
	}

	if has_2mib_pages
		&& map_size > LargePageSize::SIZE as usize
		&& map_addr.is_aligned(LargePageSize::SIZE)
	{
		let size = map_size.align_down(LargePageSize::SIZE as usize);
		if let Err(num_pages) =
			paging::map_heap::<LargePageSize>(map_addr, size / LargePageSize::SIZE as usize)
		{
			map_size -= num_pages * LargePageSize::SIZE as usize;
			map_addr += num_pages * LargePageSize::SIZE as usize;
		} else {
			map_size -= size;
			map_addr += size;
		}
	}

	if map_size > BasePageSize::SIZE as usize && map_addr.is_aligned(BasePageSize::SIZE) {
		let size = map_size.align_down(BasePageSize::SIZE as usize);
		if let Err(num_pages) =
			paging::map_heap::<BasePageSize>(map_addr, size / BasePageSize::SIZE as usize)
		{
			map_size -= num_pages * BasePageSize::SIZE as usize;
			map_addr += num_pages * BasePageSize::SIZE as usize;
		} else {
			map_size -= size;
			map_addr += size;
		}
	}

	let heap_end_addr = map_addr;

	#[cfg(not(feature = "newlib"))]
	unsafe {
		ALLOCATOR.init(
			heap_start_addr.as_mut_ptr(),
			(heap_end_addr - heap_start_addr).into(),
		);
	}

	let heap_addr_range = heap_start_addr..heap_end_addr;
	info!("Heap is located at {heap_addr_range:#x?} ({map_size} Bytes unmapped)");
	#[cfg(feature = "newlib")]
	HEAP_ADDR_RANGE.set(heap_addr_range).unwrap();
}

pub(crate) fn print_information() {
	arch::mm::physicalmem::print_information();
	arch::mm::virtualmem::print_information();
}

/// Soft-deprecated in favor of `DeviceAlloc`
pub(crate) fn allocate(size: usize, no_execution: bool) -> VirtAddr {
	let size = size.align_up(BasePageSize::SIZE as usize);
	let physical_address = arch::mm::physicalmem::allocate(size).unwrap();
	let virtual_address = arch::mm::virtualmem::allocate(size).unwrap();

	let count = size / BasePageSize::SIZE as usize;
	let mut flags = PageTableEntryFlags::empty();
	flags.normal().writable();
	if no_execution {
		flags.execute_disable();
	}
	arch::mm::paging::map::<BasePageSize>(virtual_address, physical_address, count, flags);

	virtual_address
}

/// Soft-deprecated in favor of `DeviceAlloc`
pub(crate) fn deallocate(virtual_address: VirtAddr, size: usize) {
	let size = size.align_up(BasePageSize::SIZE as usize);

	if let Some(phys_addr) = arch::mm::paging::virtual_to_physical(virtual_address) {
		arch::mm::paging::unmap::<BasePageSize>(
			virtual_address,
			size / BasePageSize::SIZE as usize,
		);
		arch::mm::virtualmem::deallocate(virtual_address, size);
		arch::mm::physicalmem::deallocate(phys_addr, size);
	} else {
		panic!(
			"No page table entry for virtual address {:p}",
			virtual_address
		);
	}
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

	let virtual_address = arch::mm::virtualmem::allocate(size).unwrap();
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
		arch::mm::virtualmem::deallocate(virtual_address, size);
	} else {
		panic!(
			"No page table entry for virtual address {:p}",
			virtual_address
		);
	}
}
