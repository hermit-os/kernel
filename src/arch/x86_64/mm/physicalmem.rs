use core::sync::atomic::{AtomicUsize, Ordering};

use free_list::{AllocError, FreeList, PageLayout, PageRange};
use hermit_sync::InterruptTicketMutex;
use multiboot::information::{MemoryType, Multiboot};

use crate::arch::x86_64::kernel::{boot_info, get_fdt, get_limit, get_mbinfo};
use crate::arch::x86_64::mm::paging::{BasePageSize, PageSize};
use crate::arch::x86_64::mm::{MultibootMemory, PhysAddr, VirtAddr};
use crate::{env, mm};

pub static PHYSICAL_FREE_LIST: InterruptTicketMutex<FreeList<16>> =
	InterruptTicketMutex::new(FreeList::new());
static TOTAL_MEMORY: AtomicUsize = AtomicUsize::new(0);

const KVM_32BIT_MAX_MEM_SIZE: usize = 1 << 32;
const KVM_32BIT_GAP_SIZE: usize = 768 << 20;
const KVM_32BIT_GAP_START: usize = KVM_32BIT_MAX_MEM_SIZE - KVM_32BIT_GAP_SIZE;

fn detect_from_fdt() -> Result<(), ()> {
	let fdt = get_fdt().ok_or(())?;

	let mems = fdt.find_all_nodes("/memory");
	let all_regions = mems.map(|m| m.reg().unwrap().next().unwrap());

	let mut found_ram = false;

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
			VirtAddr(start_address)
		};

		let range = PageRange::new(start_address.as_usize(), end_address as usize).unwrap();
		let _ = TOTAL_MEMORY.fetch_add(
			(end_address - start_address.as_u64()) as usize,
			Ordering::Relaxed,
		);
		unsafe {
			PHYSICAL_FREE_LIST.lock().deallocate(range).unwrap();
		}
	}

	assert!(
		found_ram,
		"Could not find any available RAM in the Devicetree Memory Map"
	);

	Ok(())
}

fn detect_from_multiboot_info() -> Result<(), ()> {
	let mb_info = get_mbinfo().ok_or(())?.get();

	let mut mem = MultibootMemory;
	let mb = unsafe { Multiboot::from_ptr(mb_info, &mut mem).unwrap() };
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

		let range = PageRange::new(
			start_address.as_usize(),
			(m.base_address() + m.length()) as usize,
		)
		.unwrap();
		let _ = TOTAL_MEMORY.fetch_add(
			(m.base_address() + m.length() - start_address.as_u64()) as usize,
			Ordering::Relaxed,
		);
		unsafe {
			PHYSICAL_FREE_LIST.lock().deallocate(range).unwrap();
		}
	}

	assert!(
		found_ram,
		"Could not find any available RAM in the Multiboot Memory Map"
	);

	Ok(())
}

fn detect_from_uhyve() -> Result<(), ()> {
	if !env::is_uhyve() {
		return Err(());
	}

	let physmem_end = get_limit();
	assert_ne!(physmem_end, 0);
	let mut free_list = PHYSICAL_FREE_LIST.lock();
	let total_memory;

	let kernel_end = mm::kernel_end_address().as_usize();
	// add gap for the APIC
	assert!(
		!(KVM_32BIT_GAP_START..=KVM_32BIT_GAP_START + KVM_32BIT_GAP_SIZE).contains(&kernel_end),
		"Kernel was loaded into the KVM 32BIT GAP"
	);
	if physmem_end > KVM_32BIT_GAP_START && kernel_end < KVM_32BIT_GAP_START {
		let range = PageRange::new(kernel_end, KVM_32BIT_GAP_START).unwrap();
		unsafe {
			free_list.deallocate(range).unwrap();
		}
		if physmem_end > KVM_32BIT_GAP_START + KVM_32BIT_GAP_SIZE {
			let range =
				PageRange::new(KVM_32BIT_GAP_START + KVM_32BIT_GAP_SIZE, physmem_end).unwrap();
			unsafe {
				free_list.deallocate(range).unwrap();
			}
			total_memory = boot_info().hardware_info.phys_addr_range.end
				- boot_info().hardware_info.phys_addr_range.start
				- KVM_32BIT_GAP_SIZE as u64;
		} else {
			total_memory =
				KVM_32BIT_GAP_START as u64 - boot_info().hardware_info.phys_addr_range.start;
		}
	} else {
		let range = PageRange::new(kernel_end, physmem_end).unwrap();
		unsafe {
			free_list.deallocate(range).unwrap();
		}
		total_memory = boot_info().hardware_info.phys_addr_range.end
			- boot_info().hardware_info.phys_addr_range.start;
	}

	TOTAL_MEMORY.store(total_memory as usize, Ordering::Relaxed);

	Ok(())
}

pub fn init() {
	detect_from_fdt()
		.or_else(|_e| detect_from_multiboot_info())
		.or_else(|_e| detect_from_uhyve())
		.unwrap();
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

	Ok(PhysAddr(
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

	Ok(PhysAddr(
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
	assert!(
		physical_address >= PhysAddr(mm::kernel_end_address().as_u64()),
		"Physical address {physical_address:p} is not >= KERNEL_END_ADDRESS"
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

	unsafe {
		PHYSICAL_FREE_LIST.lock().deallocate(range).unwrap();
	}
}

#[allow(dead_code)]
#[cfg(not(feature = "pci"))]
pub fn reserve(physical_address: PhysAddr, size: usize) {
	assert_eq!(
		physical_address % BasePageSize::SIZE as usize,
		0,
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
