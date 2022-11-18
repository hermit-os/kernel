use ::x86_64::structures::paging::{FrameAllocator, PhysFrame};
use core::alloc::AllocError;
use core::sync::atomic::{AtomicUsize, Ordering};
use multiboot::information::{MemoryType, Multiboot};

use crate::arch::x86_64::kernel::{get_limit, get_mbinfo};
use crate::arch::x86_64::mm::paging::{BasePageSize, PageSize};
use crate::arch::x86_64::mm::MEM;
use crate::arch::x86_64::mm::{PhysAddr, VirtAddr};
use crate::mm;
use crate::mm::freelist::{FreeList, FreeListEntry};
use crate::synch::spinlock::*;

static PHYSICAL_FREE_LIST: SpinlockIrqSave<FreeList> = SpinlockIrqSave::new(FreeList::new());
static TOTAL_MEMORY: AtomicUsize = AtomicUsize::new(0);

fn detect_from_multiboot_info() -> Result<(), ()> {
	let mb_info = get_mbinfo();
	if mb_info.is_zero() {
		return Err(());
	}

	let mb = unsafe { Multiboot::from_ptr(mb_info.as_u64(), &mut MEM).unwrap() };
	let all_regions = mb
		.memory_regions()
		.expect("Could not find a memory map in the Multiboot information");
	let ram_regions = all_regions.filter(|m| {
		m.memory_type() == MemoryType::Available
			&& m.base_address() + m.length() > mm::kernel_end_address().as_u64()
	});
	let mut found_ram = false;

	for m in ram_regions {
		found_ram = true;

		let start_address = if m.base_address() <= mm::kernel_start_address().as_u64() {
			mm::kernel_end_address()
		} else {
			VirtAddr(m.base_address())
		};

		let entry = FreeListEntry::new(
			start_address.as_usize(),
			(m.base_address() + m.length()) as usize,
		);
		let _ = TOTAL_MEMORY.fetch_add((m.base_address() + m.length()) as usize, Ordering::SeqCst);
		PHYSICAL_FREE_LIST.lock().list.push_back(entry);
	}

	assert!(
		found_ram,
		"Could not find any available RAM in the Multiboot Memory Map"
	);

	Ok(())
}

fn detect_from_limits() -> Result<(), ()> {
	let apic_gap = 0xFE000000;
	let limit = get_limit();
	if limit == 0 {
		return Err(());
	}

	// add gap for the APIC
	if limit > apic_gap {
		let entry = FreeListEntry::new(mm::kernel_end_address().as_usize(), apic_gap);
		PHYSICAL_FREE_LIST.lock().list.push_back(entry);
		if limit > 0x100000000 {
			let entry = FreeListEntry::new(0x100000000, limit - 0x100000000);
			PHYSICAL_FREE_LIST.lock().list.push_back(entry);
			TOTAL_MEMORY.store(limit - (0x100000000 - apic_gap), Ordering::SeqCst);
		} else {
			TOTAL_MEMORY.store(apic_gap, Ordering::SeqCst);
		}
	} else {
		let entry = FreeListEntry::new(mm::kernel_end_address().as_usize(), limit);
		PHYSICAL_FREE_LIST.lock().list.push_back(entry);
		TOTAL_MEMORY.store(limit, Ordering::SeqCst);
	}

	Ok(())
}

pub fn init() {
	detect_from_multiboot_info()
		.or_else(|_e| detect_from_limits())
		.unwrap();
}

pub fn total_memory_size() -> usize {
	TOTAL_MEMORY.load(Ordering::SeqCst)
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

	Ok(PhysAddr(
		PHYSICAL_FREE_LIST
			.lock()
			.allocate(size, None)?
			.try_into()
			.unwrap(),
	))
}

pub struct FrameAlloc;

unsafe impl<S: x86_64::structures::paging::PageSize> FrameAllocator<S> for FrameAlloc {
	fn allocate_frame(&mut self) -> Option<PhysFrame<S>> {
		let addr = PHYSICAL_FREE_LIST
			.lock()
			.allocate(S::SIZE as usize, Some(S::SIZE as usize))
			.ok()? as u64;
		Some(PhysFrame::from_start_address(x86_64::PhysAddr::new(addr)).unwrap())
	}
}

pub fn allocate_aligned(size: usize, alignment: usize) -> Result<PhysAddr, AllocError> {
	assert!(size > 0);
	assert!(alignment > 0);
	assert_eq!(
		size % alignment,
		0,
		"Size {:#X} is not a multiple of the given alignment {:#X}",
		size,
		alignment
	);
	assert_eq!(
		alignment % BasePageSize::SIZE as usize,
		0,
		"Alignment {:#X} is not a multiple of {:#X}",
		alignment,
		BasePageSize::SIZE
	);

	Ok(PhysAddr(
		PHYSICAL_FREE_LIST
			.lock()
			.allocate(size, Some(alignment))?
			.try_into()
			.unwrap(),
	))
}

/// This function must only be called from mm::deallocate!
/// Otherwise, it may fail due to an empty node pool (POOL.maintain() is called in virtualmem::deallocate)
pub fn deallocate(physical_address: PhysAddr, size: usize) {
	assert!(
		physical_address >= PhysAddr(mm::kernel_end_address().as_u64()),
		"Physical address {:#X} is not >= KERNEL_END_ADDRESS",
		physical_address
	);
	assert!(size > 0);
	assert_eq!(
		size % BasePageSize::SIZE as usize,
		0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE
	);

	PHYSICAL_FREE_LIST
		.lock()
		.deallocate(physical_address.as_usize(), size);
}

#[cfg(not(feature = "pci"))]
pub fn reserve(physical_address: PhysAddr, size: usize) {
	assert_eq!(
		physical_address % BasePageSize::SIZE as usize,
		0,
		"Physical address {:#X} is not a multiple of {:#X}",
		physical_address,
		BasePageSize::SIZE
	);
	assert!(size > 0);
	assert_eq!(
		size % BasePageSize::SIZE as usize,
		0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE
	);

	// we are able to ignore errors because it could be already reserved
	let _ = PHYSICAL_FREE_LIST
		.lock()
		.reserve(physical_address.as_usize(), size);
}

pub fn print_information() {
	PHYSICAL_FREE_LIST
		.lock()
		.print_information(" PHYSICAL MEMORY FREE LIST ");
}
