use core::sync::atomic::Ordering;

use align_address::Align;
use free_list::PageRange;
use memory_addresses::{PhysAddr, VirtAddr};

use crate::arch::mm::paging::{self, LargePageSize, PageSize};
use crate::mm::physicalmem::{PHYSICAL_FREE_LIST, TOTAL_MEMORY};
use crate::{env, mm};

unsafe fn init_frame_range(frame_range: PageRange) {
	let start = frame_range
		.start()
		.align_down(LargePageSize::SIZE.try_into().unwrap());
	let end = frame_range
		.end()
		.align_up(LargePageSize::SIZE.try_into().unwrap());

	unsafe {
		PHYSICAL_FREE_LIST.lock().deallocate(frame_range).unwrap();
	}

	(start..end)
		.step_by(LargePageSize::SIZE.try_into().unwrap())
		.map(|addr| PhysAddr::new(addr.try_into().unwrap()))
		.for_each(paging::identity_map::<LargePageSize>);

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
			unsafe {
				init_frame_range(range);
			}
		}
	}

	if found_ram { Ok(()) } else { Err(()) }
}

pub fn init() {
	detect_from_fdt().unwrap();
}
