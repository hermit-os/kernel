use alloc::borrow::ToOwned;
use core::ptr::write_bytes;
use core::{mem, ptr, slice};

use align_address::Align;
use hermit_entry::boot_info::{
	BootInfo, DeviceTreeAddress, HardwareInfo, PlatformInfo, SerialPortBase,
};
use hermit_entry::elf::LoadedKernel;
use log::info;
use multiboot::information::{MemoryManagement, Multiboot, PAddr};
use vm_fdt::FdtWriterResult;
use x86_64::structures::paging::{PageSize, PageTableFlags, Size2MiB, Size4KiB};

use super::paging;
use super::physicalmem::PhysAlloc;
use crate::BootInfoExt;
use crate::arch::x86_64::{KERNEL_STACK_SIZE, SERIAL_IO_PORT};
use crate::fdt::Fdt;

unsafe extern "C" {
	static mut loader_end: u8;
	static mb_info: usize;
}

#[allow(bad_asm_style)]
mod entry {
	core::arch::global_asm!(
		include_str!("entry.s"),
		loader_main = sym crate::os::loader_main,
	);
}

struct Mem;

impl MemoryManagement for Mem {
	unsafe fn paddr_to_slice<'a>(&self, p: PAddr, sz: usize) -> Option<&'static [u8]> {
		let ptr = ptr::with_exposed_provenance(p as usize);
		unsafe { Some(slice::from_raw_parts(ptr, sz)) }
	}

	// If you only want to read fields, you can simply return `None`.
	unsafe fn allocate(&mut self, _length: usize) -> Option<(PAddr, &mut [u8])> {
		None
	}

	unsafe fn deallocate(&mut self, addr: PAddr) {
		if addr != 0 {
			unimplemented!()
		}
	}
}

pub struct DeviceTree;

impl DeviceTree {
	pub fn create() -> FdtWriterResult<&'static [u8]> {
		let mut mem = Mem;
		let multiboot = unsafe { Multiboot::from_ptr(mb_info as u64, &mut mem).unwrap() };

		let memory_regions = multiboot
			.memory_regions()
			.expect("Could not find a memory map in the Multiboot information");

		let mut fdt = Fdt::new("multiboot")?.memory_regions(memory_regions)?;

		if let Some(cmdline) = multiboot.command_line() {
			fdt = fdt.bootargs(cmdline.to_owned())?;
		}

		let fdt = fdt.finish()?;

		Ok(fdt.leak())
	}
}

pub fn find_kernel() -> &'static [u8] {
	use core::cmp;

	paging::clean_up();
	// Identity-map the Multiboot information.
	unsafe {
		assert!(mb_info > 0, "Could not find Multiboot information");
		info!("Found Multiboot information at {mb_info:#x}");
		paging::map::<Size4KiB>(mb_info, mb_info, 1, PageTableFlags::empty())
	}

	let mut mem = Mem;
	// Load the Multiboot information and identity-map the modules information.
	let multiboot = unsafe { Multiboot::from_ptr(mb_info as u64, &mut mem).unwrap() };

	// Iterate through all modules.
	// Collect the start address of the first module and the highest end address of all modules.
	let mut module_iter = multiboot
		.modules()
		.expect("Could not find a memory map in the Multiboot information");

	let first_module = module_iter
		.next()
		.expect("Could not find a single module in the Multiboot information");
	info!(
		"Found an ELF module at [{:#x} - {:#x}]",
		first_module.start, first_module.end
	);
	let elf_start = first_module.start as usize;
	let elf_len = (first_module.end - first_module.start) as usize;
	info!("Module length: {elf_len:#x}");

	// Find the maximum end address from the remaining modules
	let mut end_address = first_module.end;
	for m in module_iter {
		end_address = cmp::max(end_address, m.end);
	}

	let modules_mapping_end = end_address.align_up(Size2MiB::SIZE) as usize;
	// TODO: Workaround for https://github.com/hermitcore/loader/issues/96
	let free_memory_address = cmp::max(modules_mapping_end, 0x800000);
	// Memory after the highest end address is unused and available for the physical memory manager.
	PhysAlloc::init(free_memory_address);

	// Identity-map the ELF header of the first module and until the 2 MiB
	// mapping starts. We cannot start the 2 MiB mapping right from
	// `first_module.end` because when it is aligned down, the
	// resulting mapping range may overlap with the 4 KiB mapping.
	let first_module_mapping_end = first_module.start.align_up(Size2MiB::SIZE) as usize;
	paging::map_range::<Size4KiB>(
		first_module.start as usize,
		first_module.start as usize,
		first_module_mapping_end,
		PageTableFlags::empty(),
	);

	// map also the rest of the modules
	paging::map_range::<Size2MiB>(
		first_module_mapping_end,
		first_module_mapping_end,
		modules_mapping_end,
		PageTableFlags::empty(),
	);

	unsafe { slice::from_raw_parts(ptr::with_exposed_provenance(elf_start), elf_len) }
}

pub unsafe fn boot_kernel(kernel_info: LoadedKernel) -> ! {
	let LoadedKernel {
		load_info,
		entry_point,
	} = kernel_info;

	let mut mem = Mem;
	let multiboot = unsafe { Multiboot::from_ptr(mb_info as u64, &mut mem).unwrap() };

	// determine boot stack address
	let mut new_stack = ptr::addr_of!(loader_end)
		.addr()
		.align_up(Size4KiB::SIZE as usize);

	if new_stack + KERNEL_STACK_SIZE as usize > unsafe { mb_info } {
		new_stack = (unsafe { mb_info } + mem::size_of::<Multiboot<'_, '_>>())
			.align_up(Size4KiB::SIZE as usize);
	}

	let command_line = multiboot.command_line();
	if let Some(command_line) = command_line {
		let cmdline = command_line.as_ptr() as usize;
		let cmdsize = command_line.len();
		if new_stack + KERNEL_STACK_SIZE as usize > cmdline {
			new_stack = (cmdline + cmdsize).align_up(Size4KiB::SIZE as usize);
		}
	}

	// map stack in the address space
	paging::map::<Size4KiB>(
		new_stack,
		new_stack,
		KERNEL_STACK_SIZE as usize / Size4KiB::SIZE as usize,
		PageTableFlags::WRITABLE,
	);

	let stack = ptr::addr_of_mut!(loader_end).with_addr(new_stack);

	// clear stack
	unsafe {
		write_bytes(stack, 0, KERNEL_STACK_SIZE.try_into().unwrap());
	}

	let device_tree = DeviceTree::create().expect("Unable to create devicetree!");
	let device_tree =
		DeviceTreeAddress::new(u64::try_from(device_tree.as_ptr().expose_provenance()).unwrap());

	let boot_info = BootInfo {
		hardware_info: HardwareInfo {
			phys_addr_range: 0..0,
			serial_port_base: SerialPortBase::new(SERIAL_IO_PORT),
			device_tree,
		},
		load_info,
		platform_info: PlatformInfo::Multiboot {
			command_line,
			multiboot_info_addr: (unsafe { mb_info } as u64).try_into().unwrap(),
		},
	};

	let entry = ptr::with_exposed_provenance(entry_point.try_into().unwrap());
	let raw_boot_info = boot_info.write();

	unsafe { super::enter_kernel(stack, entry, raw_boot_info) }
}
