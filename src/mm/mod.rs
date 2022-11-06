pub mod allocator;
pub mod freelist;
mod hole;
#[cfg(test)]
mod test;

use crate::arch;
#[cfg(target_arch = "x86_64")]
use crate::arch::mm::paging::PageTableEntryFlagsExt;
use crate::arch::mm::paging::{
	BasePageSize, HugePageSize, LargePageSize, PageSize, PageTableEntryFlags,
};
use crate::arch::mm::physicalmem::total_memory_size;
#[cfg(feature = "newlib")]
use crate::arch::mm::virtualmem::kernel_heap_end;
#[cfg(feature = "pci")]
use crate::arch::mm::PhysAddr;
use crate::arch::mm::VirtAddr;
use crate::env;
use core::mem;

/// Physical and virtual address of the first 2 MiB page that maps the kernel.
/// Can be easily accessed through kernel_start_address()
static mut KERNEL_START_ADDRESS: VirtAddr = VirtAddr::zero();

/// Physical and virtual address of the first page after the kernel.
/// Can be easily accessed through kernel_end_address()
static mut KERNEL_END_ADDRESS: VirtAddr = VirtAddr::zero();

/// Start address of the user heap
static mut HEAP_START_ADDRESS: VirtAddr = VirtAddr::zero();

/// End address of the user heap
static mut HEAP_END_ADDRESS: VirtAddr = VirtAddr::zero();

pub fn kernel_start_address() -> VirtAddr {
	unsafe { KERNEL_START_ADDRESS }
}

pub fn kernel_end_address() -> VirtAddr {
	unsafe { KERNEL_END_ADDRESS }
}

#[cfg(feature = "newlib")]
pub fn task_heap_start() -> VirtAddr {
	unsafe { HEAP_START_ADDRESS }
}

#[cfg(feature = "newlib")]
pub fn task_heap_end() -> VirtAddr {
	unsafe { HEAP_END_ADDRESS }
}

fn map_heap<S: PageSize>(virt_addr: VirtAddr, size: usize) {
	assert_eq!(align_down!(size, S::SIZE as usize), size);

	let flags = {
		let mut flags = PageTableEntryFlags::empty();
		flags.normal().writable().execute_disable();
		flags
	};

	let virt_addrs = (virt_addr.as_usize()..virt_addr.as_usize() + size)
		.step_by(S::SIZE as usize)
		.map(VirtAddr::from_usize);

	for virt_addr in virt_addrs {
		let phys_addr =
			arch::mm::physicalmem::allocate_aligned(S::SIZE as usize, S::SIZE as usize).unwrap();
		arch::mm::paging::map::<S>(virt_addr, phys_addr, 1, flags);
	}
}

#[cfg(target_os = "none")]
pub fn init() {
	// Calculate the start and end addresses of the 2 MiB page(s) that map the kernel.
	unsafe {
		KERNEL_START_ADDRESS = env::get_base_address().align_down_to_large_page();
		KERNEL_END_ADDRESS =
			(env::get_base_address() + env::get_image_size()).align_up_to_large_page();
	}

	arch::mm::init();
	arch::mm::init_page_tables();

	info!("Total memory size: {} MB", total_memory_size() >> 20);
	info!(
		"Kernel region: [{:#x} - {:#x}]",
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
		+ LargePageSize::SIZE as usize;
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

	let available_memory = align_down!(
		total_memory_size()
			- (kernel_end_address().as_usize() - env::get_ram_address().as_usize())
			- reserved_space,
		LargePageSize::SIZE as usize
	);

	// we reserve 10% of the memory for stack allocations
	let stack_reserve: usize = (available_memory * 10) / 100;

	#[cfg(feature = "newlib")]
	{
		info!("An application with a C-based runtime is running on top of HermitCore!");
		let kernel_heap_size = 10 * LargePageSize::SIZE;

		unsafe {
			let start = allocate(kernel_heap_size, true);
			crate::ALLOCATOR
				.lock()
				.init(start.as_usize(), kernel_heap_size);

			info!("Kernel heap starts at {:#x}", start);
		}

		info!("Kernel heap size: {} MB", kernel_heap_size >> 20);
		let user_heap_size = align_down!(
			available_memory - kernel_heap_size - stack_reserve - LargePageSize::SIZE,
			LargePageSize::SIZE
		);
		info!("User-space heap size: {} MB", user_heap_size >> 20);

		map_addr = kernel_heap_end();
		map_size = user_heap_size;
		unsafe {
			HEAP_START_ADDRESS = map_addr;
		}
	}

	#[cfg(not(feature = "newlib"))]
	{
		info!("A pure Rust application is running on top of HermitCore!");

		// At first, we map only a small part into the heap.
		// Afterwards, we already use the heap and map the rest into
		// the virtual address space.

		let virt_size: usize = align_down!(
			available_memory - stack_reserve,
			LargePageSize::SIZE as usize
		);

		let virt_addr = if has_1gib_pages && virt_size > HugePageSize::SIZE as usize {
			arch::mm::virtualmem::allocate_aligned(
				align_up!(virt_size, HugePageSize::SIZE as usize),
				HugePageSize::SIZE as usize,
			)
			.unwrap()
		} else {
			arch::mm::virtualmem::allocate_aligned(virt_size, LargePageSize::SIZE as usize).unwrap()
		};

		info!(
			"Heap: size {} MB, start address {:#x}",
			virt_size >> 20,
			virt_addr
		);

		// try to map a huge page
		let mut counter = if has_1gib_pages && virt_size > HugePageSize::SIZE as usize {
			map_heap::<HugePageSize>(virt_addr, HugePageSize::SIZE as usize);
			HugePageSize::SIZE as usize
		} else {
			0
		};

		if counter == 0 && has_2mib_pages {
			// fall back to large pages
			map_heap::<LargePageSize>(virt_addr, LargePageSize::SIZE as usize);
			counter = LargePageSize::SIZE as usize;
		}

		if counter == 0 {
			// fall back to normal pages, but map at least the size of a large page
			map_heap::<BasePageSize>(virt_addr, LargePageSize::SIZE as usize);
			counter = LargePageSize::SIZE as usize;
		}

		unsafe {
			HEAP_START_ADDRESS = virt_addr;
			crate::ALLOCATOR
				.lock()
				.init(virt_addr.as_usize(), virt_size);
		}

		map_addr = virt_addr + counter;
		map_size = virt_size - counter;
	}

	if has_1gib_pages
		&& map_size > HugePageSize::SIZE as usize
		&& align_down!(map_addr.as_usize(), HugePageSize::SIZE as usize) == 0
	{
		let size = align_down!(map_size, HugePageSize::SIZE as usize);
		map_heap::<HugePageSize>(map_addr, size);
		map_size -= size;
		map_addr += size;
	}

	if has_2mib_pages && map_size > LargePageSize::SIZE as usize {
		let size = align_down!(map_size, LargePageSize::SIZE as usize);
		map_heap::<LargePageSize>(map_addr, size);
		map_size -= size;
		map_addr += size;
	}

	if map_size > BasePageSize::SIZE as usize {
		let size = align_down!(map_size, BasePageSize::SIZE as usize);
		map_heap::<BasePageSize>(map_addr, size);
		map_size -= size;
		map_addr += size;
	}

	unsafe {
		HEAP_END_ADDRESS = map_addr;

		info!(
			"Heap is located at {:#x} -- {:#x} ({} Bytes unmapped)",
			HEAP_START_ADDRESS, HEAP_END_ADDRESS, map_size
		);
	}
}

pub fn print_information() {
	arch::mm::physicalmem::print_information();
	arch::mm::virtualmem::print_information();
}

pub fn allocate(sz: usize, no_execution: bool) -> VirtAddr {
	let size = align_up!(sz, BasePageSize::SIZE as usize);
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

pub fn deallocate(virtual_address: VirtAddr, sz: usize) {
	let size = align_up!(sz, BasePageSize::SIZE as usize);

	if let Some(entry) = arch::mm::paging::get_page_table_entry::<BasePageSize>(virtual_address) {
		arch::mm::paging::unmap::<BasePageSize>(
			virtual_address,
			size / BasePageSize::SIZE as usize,
		);
		arch::mm::virtualmem::deallocate(virtual_address, size);
		arch::mm::physicalmem::deallocate(entry.address(), size);
	} else {
		panic!(
			"No page table entry for virtual address {:#X}",
			virtual_address
		);
	}
}

/// Maps a given physical address and size in virtual space and returns address.
#[cfg(feature = "pci")]
pub fn map(
	physical_address: PhysAddr,
	sz: usize,
	writable: bool,
	no_execution: bool,
	no_cache: bool,
) -> VirtAddr {
	let size = align_up!(sz, BasePageSize::SIZE as usize);
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
pub fn unmap(virtual_address: VirtAddr, sz: usize) {
	let size = align_up!(sz, BasePageSize::SIZE as usize);

	if arch::mm::paging::get_page_table_entry::<BasePageSize>(virtual_address).is_some() {
		arch::mm::paging::unmap::<BasePageSize>(
			virtual_address,
			size / BasePageSize::SIZE as usize,
		);
		arch::mm::virtualmem::deallocate(virtual_address, size);
	} else {
		panic!(
			"No page table entry for virtual address {:#X}",
			virtual_address
		);
	}
}
