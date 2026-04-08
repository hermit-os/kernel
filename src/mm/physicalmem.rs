use core::alloc::AllocError;
use core::fmt;
use core::sync::atomic::{AtomicUsize, Ordering};

use align_address::Align;
use free_list::{FreeList, PageLayout, PageRange, PageRangeError};
use hermit_sync::InterruptTicketMutex;
use memory_addresses::{PhysAddr, VirtAddr};

#[cfg(target_arch = "x86_64")]
use crate::arch::mm::paging::PageTableEntryFlagsExt;
use crate::arch::mm::paging::{self, HugePageSize, PageSize, PageTableEntryFlags};
use crate::env;
use crate::mm::device_alloc::DeviceAlloc;
use crate::mm::{PageRangeAllocator, PageRangeBox};

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

/// Copy the physical page at `src_phys` into a freshly allocated page and return its address.
#[cfg(all(target_arch = "x86_64", feature = "common-os"))]
pub fn copy_page(src_phys: PhysAddr) -> PhysAddr {
	use free_list::PageLayout;

	use crate::arch::mm::paging::{
		BasePageSize, PageSize, PageTableEntryFlags, PageTableEntryFlagsExt,
	};
	use crate::mm::{FrameAlloc, PageBox, PageRangeAllocator};

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
	crate::arch::mm::paging::map::<BasePageSize>(virt, src_phys, 1, flags);
	crate::arch::mm::paging::map::<BasePageSize>(virt + BasePageSize::SIZE, dst_phys, 1, flags);

	unsafe {
		let src = core::slice::from_raw_parts(virt.as_ptr::<u8>(), BasePageSize::SIZE as usize);
		let dst = core::slice::from_raw_parts_mut(
			(virt + BasePageSize::SIZE).as_mut_ptr::<u8>(),
			BasePageSize::SIZE as usize,
		);
		dst.copy_from_slice(src);
	}

	crate::arch::mm::paging::unmap::<BasePageSize>(virt, 2);
	// page_box is dropped here, freeing the virtual memory

	dst_phys
}

pub fn total_memory_size() -> usize {
	TOTAL_MEMORY.load(Ordering::Relaxed)
}

pub unsafe fn map_frame_range(frame_range: PageRange) {
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

unsafe fn detect_from_fdt() -> Result<(), ()> {
	let fdt = env::fdt().ok_or(())?;

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
		let start_address = m.starting_address.expose_provenance() as u64;
		let size = m.size.unwrap() as u64;
		let end_address = start_address + size;

		if end_address <= super::kernel_end_address().as_u64() && !env::is_uefi() {
			continue;
		}

		let start_address =
			if start_address <= super::kernel_start_address().as_u64() && !env::is_uefi() {
				super::kernel_end_address()
			} else {
				VirtAddr::new(start_address)
			};

		let range = PageRange::new(start_address.as_usize(), end_address as usize).unwrap();
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

	let kernel_start = if env::is_uefi() {
		super::kernel_start_address().as_usize()
	} else {
		// FIXME: Memory before the kernel causes trouble on non-uefi systems.
		// It is unclear, which exact regions cause problems.
		0
	};
	let kernel_end = super::kernel_end_address().as_usize();
	let kernel_region = PageRange::new(kernel_start, kernel_end).unwrap();
	reserve(kernel_region);

	let fdt_start = env::fdt_addr().unwrap().get();
	let fdt_end = fdt_start + fdt.total_size();
	let fdt_region = PageRange::containing(fdt_start, fdt_end).unwrap();
	reserve(fdt_region);

	Ok(())
}

// FIXME: upstream these
trait PageRangeExt: Sized {
	fn containing(start: usize, end: usize) -> Result<Self, PageRangeError>;

	fn and(self, rhs: Self) -> Option<Self>;
}

impl PageRangeExt for PageRange {
	fn containing(start: usize, end: usize) -> Result<Self, PageRangeError> {
		let start = start.align_down(free_list::PAGE_SIZE);
		let end = end.align_up(free_list::PAGE_SIZE);
		Self::new(start, end)
	}

	fn and(self, rhs: Self) -> Option<Self> {
		let start = self.start().max(rhs.start());
		let end = self.end().min(rhs.end());
		Self::new(start, end).ok()
	}
}

unsafe fn init() {
	if env::is_uefi() && DeviceAlloc.phys_offset() != VirtAddr::zero() {
		let start = DeviceAlloc.phys_offset();
		let count = DeviceAlloc.phys_offset().as_u64() / HugePageSize::SIZE;
		let count = usize::try_from(count).unwrap();
		paging::unmap::<HugePageSize>(start, count);
	}

	unsafe {
		detect_from_fdt().unwrap();
	}
}
