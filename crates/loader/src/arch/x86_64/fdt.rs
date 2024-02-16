use alloc::format;

use multiboot::information::{MemoryType, Multiboot};
use vm_fdt::{Error as FdtError, FdtWriter};

use super::{mb_info, MEM};

pub struct DeviceTree;

impl DeviceTree {
	#[cfg(all(target_os = "none", not(feature = "fc")))]
	pub fn create() -> Result<&'static [u8], FdtError> {
		let multiboot = unsafe { Multiboot::from_ptr(mb_info as u64, &mut MEM).unwrap() };

		let all_regions = multiboot
			.memory_regions()
			.expect("Could not find a memory map in the Multiboot information");
		let ram_regions = all_regions.filter(|m| m.memory_type() == MemoryType::Available);

		let mut fdt = FdtWriter::new()?;

		let root_node = fdt.begin_node("")?;
		fdt.property_string("compatible", "linux,dummy-virt")?;
		fdt.property_u32("#address-cells", 0x2)?;
		fdt.property_u32("#size-cells", 0x2)?;

		if let Some(cmdline) = multiboot.command_line() {
			let chosen_node = fdt.begin_node("chosen")?;
			fdt.property_string("bootargs", cmdline)?;
			fdt.end_node(chosen_node)?;
		}

		for m in ram_regions {
			let start_address = m.base_address();
			let length = m.length();

			let memory_node = fdt.begin_node(format!("memory@{:x}", start_address).as_str())?;
			fdt.property_string("device_type", "memory")?;
			fdt.property_array_u64("reg", &[start_address, length])?;
			fdt.end_node(memory_node)?;
		}

		fdt.end_node(root_node)?;

		let fdt = fdt.finish()?;

		Ok(fdt.leak())
	}
}
