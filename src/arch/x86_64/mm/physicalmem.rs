use core::sync::atomic::{AtomicUsize, Ordering};

use free_list::{AllocError, FreeList, PageLayout, PageRange};
use hermit_sync::InterruptTicketMutex;
use memory_addresses::{PhysAddr, VirtAddr};

use crate::arch::x86_64::kernel::get_limit;
use crate::arch::x86_64::mm::paging::{BasePageSize, PageSize};
use crate::{env, mm};

pub static PHYSICAL_FREE_LIST: InterruptTicketMutex<FreeList<16>> =
	InterruptTicketMutex::new(FreeList::new());
static TOTAL_MEMORY: AtomicUsize = AtomicUsize::new(0);

const KVM_32BIT_MAX_MEM_SIZE: usize = 1 << 32;
const KVM_32BIT_GAP_SIZE: usize = 768 << 20;
const KVM_32BIT_GAP_START: usize = KVM_32BIT_MAX_MEM_SIZE - KVM_32BIT_GAP_SIZE;

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

		TOTAL_MEMORY.fetch_add(range.len().get(), Ordering::Relaxed);
		unsafe {
			PHYSICAL_FREE_LIST.lock().deallocate(range).unwrap();
		}
	} else {
		for m in all_regions {
			let start_address = m.starting_address as u64;
			let size = m.size.unwrap() as u64;
			let end_address = start_address + size;

			if end_address <= mm::kernel_end_address().as_u64() {
				continue;
			}

			found_ram = true;

			let start_address = if start_address <= mm::kernel_start_address().as_u64() {
				mm::kernel_end_address()
			} else {
				VirtAddr::new(start_address)
			};

			let range = PageRange::new(start_address.as_usize(), end_address as usize).unwrap();
			TOTAL_MEMORY.fetch_add(range.len().get(), Ordering::Relaxed);
			unsafe {
				PHYSICAL_FREE_LIST.lock().deallocate(range).unwrap();
			}
		}
	}

	if found_ram { Ok(()) } else { Err(()) }
}

fn detect_from_uhyve() -> Result<(), ()> {
	if !env::is_uhyve() {
		return Err(());
	}

	let limit = get_limit();
	assert_ne!(limit, 0);
	let mut free_list = PHYSICAL_FREE_LIST.lock();
	let total_memory;

	// add gap for the APIC
	if limit > KVM_32BIT_GAP_START {
		let range = PageRange::new(
			mm::kernel_end_address().as_u64() as usize,
			KVM_32BIT_GAP_START,
		)
		.unwrap();
		unsafe {
			free_list.deallocate(range).unwrap();
		}
		if limit > KVM_32BIT_GAP_START + KVM_32BIT_GAP_SIZE {
			let range = PageRange::new(KVM_32BIT_GAP_START + KVM_32BIT_GAP_SIZE, limit).unwrap();
			unsafe {
				free_list.deallocate(range).unwrap();
			}
			total_memory = limit - KVM_32BIT_GAP_SIZE;
		} else {
			total_memory = KVM_32BIT_GAP_START;
		}
	} else {
		let range = PageRange::new(mm::kernel_end_address().as_u64() as usize, limit).unwrap();
		unsafe {
			free_list.deallocate(range).unwrap();
		}
		total_memory = limit;
	}

	TOTAL_MEMORY.store(total_memory, Ordering::Relaxed);

	Ok(())
}

pub fn init() {
	detect_from_fdt().or_else(|_e| detect_from_uhyve()).unwrap();
}

pub fn total_memory_size() -> usize {
	TOTAL_MEMORY.load(Ordering::Relaxed)
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
	assert!(size > 0);
	assert_eq!(
		size % BasePageSize::SIZE as usize,
		0,
		"Size {:#X} is not a multiple of {:#X}",
		size,
		BasePageSize::SIZE
	);

	let range = PageRange::from_start_len(physical_address.as_u64() as usize, size).unwrap();

	unsafe {
		PHYSICAL_FREE_LIST.lock().deallocate(range).unwrap();
	}
}

#[allow(dead_code)]
#[cfg(not(feature = "pci"))]
pub fn reserve(physical_address: PhysAddr, size: usize) {
	use align_address::Align;
	assert!(
		physical_address.is_aligned_to(BasePageSize::SIZE),
		"Physical address {:p} is not a multiple of {:#X}",
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

	let range = PageRange::from_start_len(physical_address.as_usize(), size).unwrap();

	// FIXME: Don't ignore errors anymore
	PHYSICAL_FREE_LIST.lock().allocate_at(range).ok();
}

pub fn print_information() {
	let free_list = PHYSICAL_FREE_LIST.lock();
	info!("Physical memory free list:\n{free_list}");
}
