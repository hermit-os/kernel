use alloc::collections::LinkedList;
use core::ffi::{c_int, c_void};

use align_address::Align;
use free_list::{PageLayout, PageRange, PageRangeSub};
use hermit_sync::InterruptSpinMutex;
#[cfg(not(feature = "common-os"))]
use memory_addresses::PhysAddr;
use memory_addresses::VirtAddr;

use crate::arch;
#[cfg(target_arch = "x86_64")]
use crate::arch::mm::paging::PageTableEntryFlagsExt;
use crate::arch::mm::paging::{BasePageSize, PageSize, PageTableEntryFlags};
use crate::mm::physicalmem::PHYSICAL_FREE_LIST;
use crate::mm::virtualmem::KERNEL_FREE_LIST;
use crate::syscalls::Errno;

#[derive(Debug, Copy, Clone)]
struct MemoryRegions {
	/// Page range of the memory region.
	pub page_range: PageRange,
	/// The protection flags for the memory region.
	pub prot_flags: MemoryProtection,
}

static MEMORY_REGIONS: InterruptSpinMutex<LinkedList<MemoryRegions>> =
	InterruptSpinMutex::new(LinkedList::new());

bitflags! {
	#[repr(transparent)]
	#[derive(Debug, Copy, Clone, Default, Eq, PartialEq)]
	pub struct MemoryProtection: u32 {
		/// Pages may not be accessed.
		const None = 0;
		/// Indicates that the memory region should be readable.
		const Read = 1 << 0;
		/// Indicates that the memory region should be writable.
		const Write = 1 << 1;
		/// Indicates that the memory region should be executable.
		const Exec = 1 << 2;
	}
}

/// Creates a new virtual memory mapping of the `size` specified with
/// protection bits specified in `prot_flags`.
#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub extern "C" fn sys_mmap(size: usize, prot_flags: MemoryProtection, ret: &mut *mut u8) -> i32 {
	let size = size.align_up(BasePageSize::SIZE as usize);

	if (*ret).is_null() {
		let layout = PageLayout::from_size(size).unwrap();
		let page_range = KERNEL_FREE_LIST.lock().allocate(layout).unwrap();

		let memory_region = MemoryRegions {
			page_range,
			prot_flags,
		};

		MEMORY_REGIONS.lock().push_back(memory_region);

		let virtual_address = VirtAddr::from(page_range.start());
		debug!("Mmap {virtual_address:X} ({size}) -> {prot_flags:?})");

		*ret = virtual_address.as_mut_ptr();

		0
	} else {
		let virtual_address = VirtAddr::new(*ret as u64);
		let virtual_address = virtual_address.align_down(BasePageSize::SIZE);
		let current_range =
			PageRange::from_start_len(virtual_address.as_usize(), BasePageSize::SIZE as usize)
				.unwrap();
		let mut memory_regions = MEMORY_REGIONS.lock();

		for region in memory_regions.iter_mut() {
			if region.page_range.contains(current_range) {
				let page_range =
					PageRange::from_start_len(virtual_address.as_usize(), size).unwrap();
				let sub_page_range = region.page_range - page_range;
				let original_flags = region.prot_flags;
				region.page_range = page_range;
				region.prot_flags = prot_flags;

				match sub_page_range {
					PageRangeSub::One(range) => {
						let memory_region = MemoryRegions {
							page_range: range,
							prot_flags: original_flags,
						};
						memory_regions.push_back(memory_region);
					}
					PageRangeSub::Two(range1, range2) => {
						let memory_region = MemoryRegions {
							page_range: range1,
							prot_flags: original_flags,
						};
						memory_regions.push_back(memory_region);
						let memory_region = MemoryRegions {
							page_range: range2,
							prot_flags: original_flags,
						};
						memory_regions.push_back(memory_region);
					}
					PageRangeSub::None => {}
				}

				return 0;
			}
		}

		debug!(
			"Try reserve memory region at {:X} with size {size}",
			virtual_address.as_usize()
		);
		let page_range = PageRange::from_start_len(*ret as usize, size).unwrap();
		if KERNEL_FREE_LIST.lock().allocate_at(page_range).is_ok() {
			let memory_region = MemoryRegions {
				page_range,
				prot_flags,
			};

			MEMORY_REGIONS.lock().push_back(memory_region);

			0
		} else {
			error!(
				"Unable to reserve memory region at {:X} with size {size}",
				virtual_address.as_usize()
			);
			-i32::from(Errno::Fault)
		}
	}
}

#[cfg(not(feature = "common-os"))]
pub(crate) fn resolve_page_fault(virtual_address: VirtAddr) -> Result<(), ()> {
	let virtual_address = virtual_address.align_down(BasePageSize::SIZE);
	let current_range =
		PageRange::from_start_len(virtual_address.as_usize(), BasePageSize::SIZE as usize).unwrap();
	let mut memory_regions = MEMORY_REGIONS.lock();

	for region in memory_regions.iter_mut() {
		if region.page_range.contains(current_range) {
			if region.prot_flags.is_empty() {
				error!(
					"Memory region {region:X?} has no protection flags set, cannot resolve page fault"
				);
				return Err(());
			}

			let frame_layout =
				PageLayout::from_size(BasePageSize::SIZE.try_into().unwrap()).unwrap();
			let frame_range = PHYSICAL_FREE_LIST.lock().allocate(frame_layout).unwrap();
			let physical_address = PhysAddr::from(frame_range.start());

			if !region.prot_flags.contains(MemoryProtection::Write) {
				let mut flags = PageTableEntryFlags::empty();
				flags.normal().writable();

				arch::mm::paging::map::<BasePageSize>(virtual_address, physical_address, 1, flags);

				let slice = unsafe {
					alloc::slice::from_raw_parts_mut(
						virtual_address.as_mut_ptr(),
						BasePageSize::SIZE as usize,
					)
				};
				for byte in slice.iter_mut() {
					*byte = 0; // Initialize the page to zero
				}
			}

			let mut flags = PageTableEntryFlags::empty();
			flags.normal();
			if region.prot_flags.is_empty() {
				flags.writable();
			}
			if region.prot_flags.contains(MemoryProtection::Write) {
				flags.writable();
			}
			if !region.prot_flags.contains(MemoryProtection::Exec) {
				flags.execute_disable();
			}
			arch::mm::paging::map::<BasePageSize>(virtual_address, physical_address, 1, flags);

			if region.prot_flags.contains(MemoryProtection::Write) {
				let slice = unsafe {
					alloc::slice::from_raw_parts_mut(
						virtual_address.as_mut_ptr(),
						BasePageSize::SIZE as usize,
					)
				};
				for byte in slice.iter_mut() {
					*byte = 0; // Initialize the page to zero
				}
			}

			return Ok(());
		}
	}

	error!(
		"Don't find memory region for address {:X}!",
		virtual_address.as_usize()
	);

	Err(())
}

/// Unmaps memory at the specified `ptr` for `size` bytes.
#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub extern "C" fn sys_munmap(ptr: *mut u8, size: usize) -> i32 {
	let size = size.align_up(BasePageSize::SIZE as usize);
	let virtual_address = VirtAddr::from_ptr(ptr);
	let page_range = PageRange::from_start_len(virtual_address.as_usize(), size).unwrap();
	let mut memory_regions = MEMORY_REGIONS.lock();

	let mut sub_page_range = None;
	let mut prot_flags = None;
	memory_regions.retain(|region| {
		if region.page_range.contains(page_range) {
			sub_page_range = Some(region.page_range - page_range);
			prot_flags = Some(region.prot_flags);

			false
		} else {
			true
		}
	});

	if let Some(sub_page_range) = sub_page_range {
		match sub_page_range {
			PageRangeSub::One(range) => {
				let memory_region = MemoryRegions {
					page_range: range,
					prot_flags: prot_flags.unwrap(),
				};
				memory_regions.push_back(memory_region);
			}
			PageRangeSub::Two(range1, range2) => {
				let memory_region1 = MemoryRegions {
					page_range: range1,
					prot_flags: prot_flags.unwrap(),
				};
				memory_regions.push_back(memory_region1);

				let memory_region2 = MemoryRegions {
					page_range: range2,
					prot_flags: prot_flags.unwrap(),
				};
				memory_regions.push_back(memory_region2);
			}
			PageRangeSub::None => {}
		}
	}

	unsafe {
		KERNEL_FREE_LIST.lock().deallocate(page_range).unwrap();
	}

	for i in 0..(size / BasePageSize::SIZE as usize) {
		if let Some(physical_address) =
			arch::mm::paging::virtual_to_physical(virtual_address + i * BasePageSize::SIZE as usize)
		{
			arch::mm::paging::unmap::<BasePageSize>(
				virtual_address + i * BasePageSize::SIZE as usize,
				1,
			);
			debug!("Unmapping {virtual_address:X} ({size}) -> {physical_address:X}");

			let range = PageRange::from_start_len(
				physical_address.as_u64() as usize,
				BasePageSize::SIZE as usize,
			)
			.unwrap();
			if let Err(_err) = unsafe { PHYSICAL_FREE_LIST.lock().deallocate(range) } {
				// FIXME: return EINVAL instead, once wasmtime can handle it
				error!("Unable to deallocate {range:?}");
			}
		}
	}

	0
}

/// Configures the protections associated with a region of virtual memory
/// starting at `ptr` and going to `size`.
///
/// Returns 0 on success and an error code on failure.
#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub extern "C" fn sys_mprotect(ptr: *mut u8, size: usize, prot_flags: MemoryProtection) -> i32 {
	let size = size.align_up(BasePageSize::SIZE as usize);
	let virtual_address = VirtAddr::from_ptr(ptr);
	let virtual_address = virtual_address.align_down(BasePageSize::SIZE);
	let current_range = PageRange::new(
		virtual_address.as_usize(),
		virtual_address.as_usize() + BasePageSize::SIZE as usize,
	)
	.unwrap();
	let mut memory_regions = MEMORY_REGIONS.lock();

	let mut found = false;
	for region in memory_regions.iter_mut() {
		if region.page_range.contains(current_range) {
			let new_range = PageRange::from_start_len(virtual_address.as_usize(), size).unwrap();

			let sub_range = region.page_range - new_range;
			let original_flags = region.prot_flags;
			found = true;
			region.page_range = new_range;
			region.prot_flags = prot_flags;

			match sub_range {
				PageRangeSub::One(range) => {
					let memory_region = MemoryRegions {
						page_range: range,
						prot_flags: original_flags,
					};
					memory_regions.push_back(memory_region);
				}
				PageRangeSub::Two(range1, range2) => {
					let memory_region = MemoryRegions {
						page_range: range1,
						prot_flags: original_flags,
					};
					memory_regions.push_back(memory_region);

					let memory_region = MemoryRegions {
						page_range: range2,
						prot_flags: original_flags,
					};
					memory_regions.push_back(memory_region);
				}
				PageRangeSub::None => {}
			}

			break;
		}
	}

	if !found {
		error!("No memory region found for {current_range:?}");
		return -i32::from(Errno::Nomem);
	}

	drop(memory_regions);

	let mut flags = PageTableEntryFlags::empty();
	flags.normal();
	if prot_flags.contains(MemoryProtection::Write) {
		flags.writable();
	}
	if !prot_flags.contains(MemoryProtection::Exec) {
		flags.execute_disable();
	}

	debug!("mprotect {virtual_address:X} ({size}) -> {prot_flags:?})");
	for i in 0..(size / BasePageSize::SIZE as usize) {
		if let Some(physical_address) =
			arch::mm::paging::virtual_to_physical(virtual_address + i * BasePageSize::SIZE as usize)
		{
			arch::mm::paging::map::<BasePageSize>(
				virtual_address + i * BasePageSize::SIZE as usize,
				physical_address,
				1,
				flags,
			);
		}
	}
	0
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub extern "C" fn sys_mlock(_addr: *const c_void, _size: usize) -> i32 {
	// Hermit does not do any swapping yet.
	0
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub extern "C" fn sys_munlock(_addr: *const c_void, _size: usize) -> i32 {
	// Hermit does not do any swapping yet.
	0
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub extern "C" fn sys_mlockall(_flags: c_int) -> i32 {
	// Hermit does not do any swapping yet.
	0
}

#[hermit_macro::system(errno)]
#[unsafe(no_mangle)]
pub extern "C" fn sys_munlockall(_flags: c_int) -> i32 {
	// Hermit does not do any swapping yet.
	0
}
