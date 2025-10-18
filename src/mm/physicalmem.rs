use alloc::alloc::AllocError;
use core::sync::atomic::{AtomicUsize, Ordering};

use align_address::Align;
use free_list::{FreeList, PageLayout, PageRange};
use hermit_sync::InterruptTicketMutex;
use memory_addresses::{PhysAddr, VirtAddr};
use smallvec::SmallVec;

#[cfg(target_arch = "x86_64")]
use crate::arch::mm::paging::PageTableEntryFlagsExt;
use crate::arch::mm::paging::{self, HugePageSize, PageSize, PageTableEntryFlags};
use crate::env::{self, is_uefi};
use crate::mm::device_alloc::DeviceAlloc;

const FREE_LIST_INLINE_SIZE: usize = 16;

static PHYSICAL_FREE_LIST: InterruptTicketMutex<FreeList<FREE_LIST_INLINE_SIZE>> =
	InterruptTicketMutex::new(FreeList::new());
pub static TOTAL_MEMORY: AtomicUsize = AtomicUsize::new(0);

pub fn total_memory_size() -> usize {
	TOTAL_MEMORY.load(Ordering::Relaxed)
}

/// Allocate physical memory.
pub fn allocate_physical(size: usize, align: usize) -> Result<PhysAddr, AllocError> {
	let page_range = PHYSICAL_FREE_LIST
		.lock()
		.allocate(PageLayout::from_size_align(size, align).unwrap())
		.map_err(|_| AllocError)?;
	Ok(PhysAddr::new(page_range.start() as u64))
}

/// Deallocate memory previously allocated with [allocate_physical].
pub unsafe fn deallocate_physical(addr: PhysAddr, size: usize) {
	unsafe {
		PHYSICAL_FREE_LIST
			.lock()
			.deallocate(PageRange::new(addr.as_u64() as usize, size).unwrap())
			.unwrap();
	};
}

pub fn print_physical_free_list() {
	info!("Physical memory free list:\n{}", PHYSICAL_FREE_LIST.lock());
}

pub unsafe fn init_frame_range(frame_range: PageRange) {
	cfg_if::cfg_if! {
		if #[cfg(target_arch = "aarch64")] {
			type IdentityPageSize = crate::arch::mm::paging::BasePageSize;
		} else if #[cfg(target_arch = "riscv64")] {
			type IdentityPageSize = crate::arch::mm::paging::HugePageSize;
		} else if #[cfg(target_arch = "x86_64")] {
			type IdentityPageSize = crate::arch::mm::paging::LargePageSize;
		}
	}

	// FIXME: rounding outwards adds physical memory that is not actually available.
	// This seems wrong.
	let start = frame_range
		.start()
		.align_down(IdentityPageSize::SIZE.try_into().unwrap());
	let end = frame_range
		.end()
		.align_up(IdentityPageSize::SIZE.try_into().unwrap());

	unsafe {
		PHYSICAL_FREE_LIST.lock().deallocate(frame_range).unwrap();
	}

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

	debug!(
		"claimed physical memory: 0x{:x}..0x{:x},",
		frame_range.start(),
		frame_range.end()
	);
	TOTAL_MEMORY.fetch_add(frame_range.len().get(), Ordering::Relaxed);
}

fn detect_from_fdt() -> Result<(), ()> {
	let fdt = env::fdt().ok_or(())?;

	let mut reserved_regions: SmallVec<[PageRange; FREE_LIST_INLINE_SIZE]> = fdt
		.memory_reservations()
		.map(|reserved| {
			let start = reserved.address() as usize;
			let end = start + reserved.size();
			PageRange::new(start, end).unwrap()
		})
		.collect();

	reserved_regions.push(
		PageRange::new(
			// FIXME: memory before the kernel causes trouble on non-uefi systems.
			// It is unclear, which exact regions cause problems
			if is_uefi() {
				super::kernel_start_address().as_usize()
			} else {
				0
			},
			super::kernel_end_address().as_usize(),
		)
		.unwrap(),
	);
	{
		let fdt_start = env::boot_info().hardware_info.device_tree.unwrap().get() as usize;
		let fdt_end = fdt_start + fdt.total_size();
		reserved_regions.push(
			PageRange::new(
				fdt_start.align_down(free_list::PAGE_SIZE),
				fdt_end.align_up(free_list::PAGE_SIZE),
			)
			.unwrap(),
		);
	}

	reserved_regions.sort_unstable_by_key(|r| r.start());
	if log_enabled!(log::Level::Debug) {
		for reserved in &reserved_regions {
			debug!(
				"reserved physical memory region: 0x{:x}..0x{:x}",
				reserved.start(),
				reserved.end()
			);
		}
	}

	let all_memories = fdt
		.find_all_nodes("/memory")
		.map(|m| m.reg().unwrap().next().unwrap());

	let mut found_ram = false;
	let mut init_range = |start: usize, end: usize| {
		let start = start.align_up(free_list::PAGE_SIZE);
		let end = end.align_down(free_list::PAGE_SIZE);
		if start < end {
			found_ram = true;
			unsafe {
				init_frame_range(PageRange::new(start, end).unwrap());
			}
		}
	};
	for memory in all_memories {
		let mut start = memory.starting_address as usize;
		let end = start + memory.size.unwrap();
		let mut reservations = reserved_regions.iter();
		while start < end {
			match reservations.next() {
				Some(reserved) => {
					if start < reserved.start() {
						// reservations are ordered by start,
						// so no reservation further down the iterator will overlap this.
						init_range(start, end.min(reserved.start()));
					}
					start = start.max(reserved.end());
				}
				None => {
					init_range(start, end);
					break;
				}
			}
		}
	}

	if found_ram { Ok(()) } else { Err(()) }
}

#[cfg(any(target_arch = "aarch64", target_arch = "riscv64"))]
fn detect_from_limits() -> Result<(), ()> {
	let limit = crate::arch::kernel::get_limit();
	if limit == 0 {
		return Err(());
	}

	#[cfg(target_arch = "riscv64")]
	let ram_address = crate::arch::kernel::get_ram_address().as_usize();
	#[cfg(target_arch = "aarch64")]
	let ram_address = 0;

	let range =
		PageRange::new(super::kernel_end_address().as_usize(), ram_address + limit).unwrap();
	unsafe {
		init_frame_range(range);
	}

	Ok(())
}

pub fn init() {
	if env::is_uefi() && DeviceAlloc.phys_offset() != VirtAddr::zero() {
		let start = DeviceAlloc.phys_offset();
		let count = DeviceAlloc.phys_offset().as_u64() / HugePageSize::SIZE;
		let count = usize::try_from(count).unwrap();
		paging::unmap::<HugePageSize>(start, count);
	}

	if let Err(_err) = detect_from_fdt() {
		cfg_if::cfg_if! {
			if #[cfg(any(target_arch = "aarch64", target_arch = "riscv64"))] {
				error!("Could not detect physical memory from FDT");
				detect_from_limits().unwrap();
			} else {
				panic!("Could not detect physical memory from FDT");
			}
		}
	}
}
