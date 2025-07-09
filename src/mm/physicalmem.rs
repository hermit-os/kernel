use core::sync::atomic::{AtomicUsize, Ordering};

use align_address::Align;
use free_list::{AllocError, FreeList, PageLayout, PageRange};
use hermit_sync::InterruptTicketMutex;
use memory_addresses::{PhysAddr, VirtAddr};

#[cfg(target_arch = "x86_64")]
use crate::arch::mm::paging::PageTableEntryFlagsExt;
use crate::arch::mm::paging::{self, BasePageSize, HugePageSize, PageSize, PageTableEntryFlags};
use crate::env;
use crate::mm::device_alloc::DeviceAlloc;

pub static PHYSICAL_FREE_LIST: InterruptTicketMutex<FreeList<16>> =
	InterruptTicketMutex::new(FreeList::new());
pub static TOTAL_MEMORY: AtomicUsize = AtomicUsize::new(0);

pub fn total_memory_size() -> usize {
	TOTAL_MEMORY.load(Ordering::Relaxed)
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

	TOTAL_MEMORY.fetch_add(frame_range.len().get(), Ordering::Relaxed);
}

fn detect_from_fdt() -> Result<(), ()> {
	let fdt = env::fdt().ok_or(())?;

	let all_regions = fdt
		.find_all_nodes("/memory")
		.map(|m| m.reg().unwrap().next().unwrap());

	let mut found_ram = false;

	if env::is_uefi() {
		let biggest_region = all_regions.max_by_key(|m| m.size.unwrap()).unwrap();
		found_ram = true;

		let range = PageRange::from_start_len(
			biggest_region.starting_address.addr(),
			biggest_region.size.unwrap(),
		)
		.unwrap();

		unsafe {
			init_frame_range(range);
		}
	} else {
		for m in all_regions {
			let start_address = m.starting_address as u64;
			let size = m.size.unwrap() as u64;
			let end_address = start_address + size;

			if end_address <= super::kernel_end_address().as_u64() {
				continue;
			}

			found_ram = true;

			let start_address = if start_address <= super::kernel_start_address().as_u64() {
				super::kernel_end_address()
			} else {
				VirtAddr::new(start_address)
			};

			let range = PageRange::new(start_address.as_usize(), end_address as usize).unwrap();
			unsafe {
				init_frame_range(range);
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

pub fn allocate(size: usize) -> Result<PhysAddr, AllocError> {
	assert!(size > 0);
	assert_eq!(
		size % BasePageSize::SIZE as usize,
		0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE
	);

	let layout = PageLayout::from_size(size).unwrap();

	Ok(PhysAddr::new(
		PHYSICAL_FREE_LIST
			.lock()
			.allocate(layout)?
			.start()
			.try_into()
			.unwrap(),
	))
}

pub fn allocate_aligned(size: usize, align: usize) -> Result<PhysAddr, AllocError> {
	assert!(size > 0);
	assert!(align > 0);
	assert_eq!(
		size % align,
		0,
		"Size {size:#X} is not a multiple of the given alignment {align:#X}"
	);
	assert_eq!(
		align % BasePageSize::SIZE as usize,
		0,
		"Alignment {:#X} is not a multiple of {:#X}",
		align,
		BasePageSize::SIZE
	);

	let layout = PageLayout::from_size_align(size, align).unwrap();

	Ok(PhysAddr::new(
		PHYSICAL_FREE_LIST
			.lock()
			.allocate(layout)?
			.start()
			.try_into()
			.unwrap(),
	))
}

/// This function must only be called from mm::deallocate!
/// Otherwise, it may fail due to an empty node pool (POOL.maintain() is called in virtualmem::deallocate)
pub fn deallocate(physical_address: PhysAddr, size: usize) {
	#[cfg(any(target_arch = "aarch64", target_arch = "riscv64"))]
	assert!(
		physical_address >= PhysAddr::new(crate::mm::kernel_end_address().as_u64()),
		"Physical address {physical_address:#X} is not >= KERNEL_END_ADDRESS"
	);
	assert!(size > 0);
	assert_eq!(
		size % BasePageSize::SIZE as usize,
		0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE
	);

	let range = PageRange::from_start_len(physical_address.as_u64() as usize, size).unwrap();
	if let Err(_err) = unsafe { PHYSICAL_FREE_LIST.lock().deallocate(range) } {
		error!("Unable to deallocate {range:?}");
	}
}
