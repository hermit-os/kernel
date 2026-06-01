#[cfg(all(feature = "common-os", feature = "fork"))]
use alloc::collections::BTreeMap;
use core::alloc::AllocError;
use core::fmt;
use core::sync::atomic::{AtomicUsize, Ordering};

use align_address::Align;
use free_list::{FreeList, PageLayout, PageRange};
use hermit_sync::InterruptTicketMutex;
use memory_addresses::VirtAddr;

#[cfg(all(target_arch = "x86_64", feature = "hermit-entry"))]
use crate::arch::mm::paging::PageTableEntryFlagsExt;
use crate::arch::mm::paging::{self, HugePageSize, PageSize};
use crate::env::{self, MemmapType, StartInfo};
use crate::mm::device_alloc::DeviceAlloc;
use crate::mm::{PageRangeAllocator, PageRangeBox};
use crate::page_range_ext::PageRangeExt;

static PHYSICAL_FREE_LIST: InterruptTicketMutex<FreeList<16>> =
	InterruptTicketMutex::new(FreeList::new());
pub static TOTAL_MEMORY: AtomicUsize = AtomicUsize::new(0);

/// Sparse per-frame COW reference counts.
/// Only frames that are actively COW-shared have an entry; exclusively-owned
/// frames are absent (equivalent to refcount 0).  Stored in a `BTreeMap` so
/// that memory use scales with the number of *shared* frames, not with total
/// physical memory.
#[cfg(all(feature = "common-os", feature = "fork"))]
static PAGE_REFCOUNTS: InterruptTicketMutex<BTreeMap<usize, u32>> =
	InterruptTicketMutex::new(BTreeMap::new());

/// Increment the COW reference count for `phys_addr` (4 KiB-aligned frame).
#[cfg(all(feature = "common-os", feature = "fork"))]
pub fn frame_ref_inc(phys_addr: PhysAddr) {
	let frame = (phys_addr.as_u64() as usize) >> 12;
	*PAGE_REFCOUNTS.lock().entry(frame).or_insert(0) += 1;
}

/// Decrement the COW reference count for `phys_addr`.
/// If the count reaches zero the the function returned true.
#[cfg(all(feature = "common-os", feature = "fork"))]
pub fn frame_ref_dec(phys_addr: PhysAddr) -> bool {
	let frame = (phys_addr.as_u64() as usize) >> 12;
	let mut map = PAGE_REFCOUNTS.lock();
	match map.get_mut(&frame) {
		None => {
			warn!("frame_ref_dec: no refcount entry for frame {phys_addr:p}");
			false
		}
		Some(count) if *count <= 1 => {
			map.remove(&frame);
			true
		}
		Some(count) => {
			*count -= 1;
			false
		}
	}
}

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

impl FrameAlloc {
	pub fn free_space() -> usize {
		PHYSICAL_FREE_LIST.lock().free_space()
	}
}

impl fmt::Display for FrameAlloc {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let free_list = PHYSICAL_FREE_LIST.lock();
		write!(f, "FrameAlloc free list:\n{free_list}")
	}
}

pub type FrameBox = PageRangeBox<FrameAlloc>;

/// Copy the physical page at `src_phys` into a freshly allocated page and return its address.
#[cfg(feature = "common-os")]
pub fn copy_page(src_phys: PhysAddr) -> PhysAddr {
	use crate::arch::mm::paging::BasePageSize;
	use crate::mm::PageBox;

	let frame_layout = PageLayout::from_size(BasePageSize::SIZE as usize).unwrap();
	let frame_range = FrameAlloc::allocate(frame_layout).expect("Failed to allocate page");
	let dst_phys = PhysAddr::new(frame_range.start().try_into().unwrap());

	let page_layout = PageLayout::from_size(2 * BasePageSize::SIZE as usize).unwrap();
	let page_box = PageBox::new(page_layout).unwrap();
	let virt = VirtAddr::from(page_box.start());

	let flags = {
		let mut flags = PageTableEntryFlags::empty();
		flags.normal().writable();
		flags
	};
	paging::map::<BasePageSize>(virt, src_phys, 1, flags);
	paging::map::<BasePageSize>(virt + BasePageSize::SIZE, dst_phys, 1, flags);

	unsafe {
		let src = core::slice::from_raw_parts(virt.as_ptr::<u8>(), BasePageSize::SIZE as usize);
		let dst = core::slice::from_raw_parts_mut(
			(virt + BasePageSize::SIZE).as_mut_ptr::<u8>(),
			BasePageSize::SIZE as usize,
		);
		dst.copy_from_slice(src);
	}

	paging::unmap::<BasePageSize>(virt, 2);
	// page_box is dropped here, freeing the virtual memory

	dst_phys
}

pub fn total_memory_size() -> usize {
	TOTAL_MEMORY.load(Ordering::Relaxed)
}

#[cfg(feature = "hermit-entry")]
pub unsafe fn map_frame_range(frame_range: PageRange) {
	use memory_addresses::PhysAddr;

	use crate::arch::mm::paging::PageTableEntryFlags;

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

unsafe fn detect_from_start_info() {
	for memmap_entry in env::start_info().memmap() {
		if memmap_entry.ty != MemmapType::Ram {
			continue;
		}

		let mut start_addr = memmap_entry.phys_addr;
		let mut end_addr = start_addr + memmap_entry.len;

		// Don't use the zero page.
		start_addr = start_addr.max(0x1000);

		#[cfg(all(target_arch = "x86_64", feature = "hermit-entry"))]
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
		}
		#[cfg(feature = "hermit-entry")]
		unsafe {
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

	let kernel_start = elf_symbols::executable_start().addr();
	let kernel_end = elf_symbols::executable_end().addr();
	let kernel_region = PageRange::containing(kernel_start, kernel_end).unwrap();
	reserve(kernel_region);

	for module in env::start_info().modules() {
		reserve(module.phys_frame_range());
	}

	#[cfg(feature = "hermit-entry")]
	{
		use crate::env::FdtStartInfo;

		let fdt = env::start_info().fdt().unwrap();

		for reservation in fdt.memory_reservations() {
			let start = reservation.address().addr();
			let end = start + reservation.size();
			let reservation = PageRange::new(start, end).unwrap();
			reserve(reservation);
		}

		let fdt_start = env::start_info().fdt_addr().unwrap().get();
		let fdt_end = fdt_start + fdt.total_size();
		let fdt_region = PageRange::containing(fdt_start, fdt_end).unwrap();
		reserve(fdt_region);
	}
}

unsafe fn init() {
	if cfg!(target_arch = "x86_64") && DeviceAlloc.phys_offset() != VirtAddr::zero() {
		let start = DeviceAlloc.phys_offset();
		let count = DeviceAlloc.phys_offset().as_u64() / HugePageSize::SIZE;
		let count = usize::try_from(count).unwrap();
		paging::unmap::<HugePageSize>(start, count);
	}

	unsafe {
		detect_from_start_info();
	}
}
