use free_list::PageRange;
use memory_addresses::VirtAddr;

use crate::{env, mm};

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
			mm::physicalmem::init_frame_range(range);
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
				mm::physicalmem::init_frame_range(range);
			}
		}
	}

	if found_ram { Ok(()) } else { Err(()) }
}

pub fn init() {
	detect_from_fdt().unwrap();
}
