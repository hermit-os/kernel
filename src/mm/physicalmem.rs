use core::alloc::AllocError;
use core::fmt;
use core::sync::atomic::{AtomicUsize, Ordering};

#[cfg(feature = "hermit-entry")]
use align_address::Align;
use free_list::{FreeList, PageLayout, PageRange};
use hermit_sync::InterruptTicketMutex;
use memory_addresses::VirtAddr;

#[cfg(feature = "hermit-entry")]
use crate::arch::mm::paging::PageTableEntryFlags;
#[cfg(all(target_arch = "x86_64", feature = "hermit-entry"))]
use crate::arch::mm::paging::PageTableEntryFlagsExt;
use crate::arch::mm::paging::{self, HugePageSize, PageSize};
#[cfg(feature = "hermit-entry")]
use crate::env::{self, FdtStartInfo, StartInfo};
use crate::mm::device_alloc::DeviceAlloc;
use crate::mm::{PageRangeAllocator, PageRangeBox};
#[cfg(feature = "hermit-entry")]
use crate::page_range_ext::PageRangeExt;

static PHYSICAL_FREE_LIST: InterruptTicketMutex<FreeList<16>> =
	InterruptTicketMutex::new(FreeList::new());
pub static TOTAL_MEMORY: AtomicUsize = AtomicUsize::new(0);

pub struct FrameAlloc;

impl PageRangeAllocator for FrameAlloc {
	unsafe fn init() {
		unsafe {
			init();
		}
	}

	fn allocate(layout: PageLayout) -> Result<PageRange, AllocError> {
		PHYSICAL_FREE_LIST
			.lock()
			.allocate(layout)
			.map_err(|_| AllocError)
	}

	fn allocate_at(range: PageRange) -> Result<(), AllocError> {
		PHYSICAL_FREE_LIST
			.lock()
			.allocate_at(range)
			.map_err(|_| AllocError)
	}

	unsafe fn deallocate(range: PageRange) {
		unsafe {
			PHYSICAL_FREE_LIST.lock().deallocate(range).unwrap();
		}
	}
}

impl fmt::Display for FrameAlloc {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let free_list = PHYSICAL_FREE_LIST.lock();
		write!(f, "FrameAlloc free list:\n{free_list}")
	}
}

pub type FrameBox = PageRangeBox<FrameAlloc>;

pub fn total_memory_size() -> usize {
	TOTAL_MEMORY.load(Ordering::Relaxed)
}

#[cfg(feature = "hermit-entry")]
pub unsafe fn map_frame_range(frame_range: PageRange) {
	use memory_addresses::PhysAddr;

	cfg_select! {
		target_arch = "aarch64" => {
			type IdentityPageSize = paging::BasePageSize;
		}
		target_arch = "riscv64" => {
			type IdentityPageSize = HugePageSize;
		}
		target_arch = "x86_64" => {
			type IdentityPageSize = paging::LargePageSize;
		}
	}

	let start = frame_range
		.start()
		.align_down(IdentityPageSize::SIZE.try_into().unwrap());
	let end = frame_range
		.end()
		.align_up(IdentityPageSize::SIZE.try_into().unwrap());

	(start..end)
		.step_by(IdentityPageSize::SIZE.try_into().unwrap())
		.map(|addr| PhysAddr::new(addr.try_into().unwrap()))
		.for_each(paging::identity_map::<IdentityPageSize>);

	// Map the physical memory again if DeviceAlloc operates at an offset
	if DeviceAlloc.phys_offset() != VirtAddr::zero() {
		let flags = {
			let mut flags = PageTableEntryFlags::empty();
			flags.normal().writable().execute_disable();
			flags
		};
		(start..end)
			.step_by(IdentityPageSize::SIZE.try_into().unwrap())
			.for_each(|addr| {
				let phys_addr = PhysAddr::new(addr.try_into().unwrap());
				let virt_addr = VirtAddr::from_ptr(DeviceAlloc.ptr_from::<()>(phys_addr));
				paging::map::<IdentityPageSize>(virt_addr, phys_addr, 1, flags);
			});
	}
}

#[cfg(feature = "hermit-entry")]
unsafe fn detect_from_fdt() -> Result<(), ()> {
	let fdt = env::start_info().fdt().ok_or(())?;

	let all_regions = fdt
		.find_all_nodes("/memory")
		.map(|m| m.reg().unwrap().next().unwrap());
	if all_regions.count() == 0 {
		return Err(());
	}
	let all_regions = fdt
		.find_all_nodes("/memory")
		.map(|m| m.reg().unwrap().next().unwrap());

	for m in all_regions {
		let mut start_addr = m.starting_address.expose_provenance();
		let mut end_addr = start_addr + m.size.unwrap();

		// Don't use the zero page.
		start_addr = start_addr.max(0x1000);

		#[cfg(target_arch = "x86_64")]
		if paging::is_recursive() {
			start_addr = start_addr.max(elf_symbols::executable_end().addr());
		}

		if cfg!(target_arch = "aarch64") || cfg!(target_arch = "riscv64") {
			start_addr = start_addr.max(elf_symbols::executable_end().addr());
		}

		start_addr = start_addr.align_up(0x1000);
		end_addr = end_addr.align_down(0x1000);

		if start_addr > end_addr {
			continue;
		}

		let range = PageRange::new(start_addr, end_addr).unwrap();
		unsafe {
			FrameAlloc::deallocate(range);
			map_frame_range(range);
		}
		TOTAL_MEMORY.fetch_add(range.len().get(), Ordering::Relaxed);
		debug!("Claimed physical memory: {range:#x?}");
	}

	let reserve = |reservation: PageRange| {
		debug!("Memory reservation: {reservation:#x?}");
		// While there are still overlaps between this reservation and any available ranges,
		// allocate that overlap to mark it as not available.
		while let Ok(reserved) = PHYSICAL_FREE_LIST
			.lock()
			.allocate_with(|range| reservation.and(range))
		{
			debug!("Reserved {reserved:#x?}");
		}
	};

	for reservation in fdt.memory_reservations() {
		let start = reservation.address().addr();
		let end = start + reservation.size();
		let reservation = PageRange::new(start, end).unwrap();
		reserve(reservation);
	}

	let kernel_start = elf_symbols::executable_start().addr();
	let kernel_end = elf_symbols::executable_end().addr();
	let kernel_region = PageRange::containing(kernel_start, kernel_end).unwrap();
	reserve(kernel_region);

	let fdt_start = env::start_info().fdt_addr().unwrap().get();
	let fdt_end = fdt_start + fdt.total_size();
	let fdt_region = PageRange::containing(fdt_start, fdt_end).unwrap();
	reserve(fdt_region);

	for module in env::start_info().modules() {
		reserve(module.phys_frame_range());
	}

	Ok(())
}

unsafe fn init() {
	if cfg!(target_arch = "x86_64") && DeviceAlloc.phys_offset() != VirtAddr::zero() {
		let start = DeviceAlloc.phys_offset();
		let count = DeviceAlloc.phys_offset().as_u64() / HugePageSize::SIZE;
		let count = usize::try_from(count).unwrap();
		paging::unmap::<HugePageSize>(start, count);
	}

	#[cfg(feature = "hermit-entry")]
	unsafe {
		detect_from_fdt().unwrap();
	}
}
