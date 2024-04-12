mod console;
pub use self::console::Console;
mod address_range;
mod start;

use core::arch::asm;
use core::{mem, slice};

use address_range::AddressRange;
use fdt::node::FdtNode;
use hermit_entry::boot_info::{
	BootInfo, DeviceTreeAddress, HardwareInfo, PlatformInfo, RawBootInfo,
};
use hermit_entry::elf::LoadedKernel;
use hermit_entry::Entry;
use log::info;
use sptr::Strict;

use crate::BootInfoExt;

fn find_kernel_linux(chosen: &FdtNode<'_, '_>) -> Option<&'static [u8]> {
	let initrd_start = chosen.property("linux,initrd-start")?.as_usize()?;
	let initrd_start = sptr::from_exposed_addr_mut::<u8>(initrd_start);
	let initrd_end = chosen.property("linux,initrd-end")?.as_usize()?;
	let initrd_end = sptr::from_exposed_addr_mut::<u8>(initrd_end);
	// SAFETY: We trust the raw pointer from the firmware
	let initrd_len = unsafe { initrd_end.offset_from(initrd_start).try_into().unwrap() };

	// SAFETY: We trust the raw pointer from the firmware
	Some(unsafe { slice::from_raw_parts(initrd_start, initrd_len) })
}

fn find_kernel_multiboot(chosen: &FdtNode<'_, '_>) -> Option<&'static [u8]> {
	let module = chosen
		.children()
		.filter(|child| child.name.starts_with("module@"))
		.find(|child| {
			child.compatible().map_or(false, |compatible| {
				compatible
					.all()
					.any(|compatible| compatible == "multiboot,ramdisk")
			})
		})?;
	let reg = module.property("reg").unwrap();
	let addr = usize::from_be_bytes(reg.value[..mem::size_of::<usize>()].try_into().unwrap());
	let len = usize::from_be_bytes(reg.value[mem::size_of::<usize>()..].try_into().unwrap());

	let initrd_start = sptr::from_exposed_addr_mut::<u8>(addr);
	// SAFETY: We trust the raw pointer from the firmware
	return Some(unsafe { slice::from_raw_parts(initrd_start, len) });
}

pub fn find_kernel() -> &'static [u8] {
	let fdt = start::get_fdt();
	let chosen = fdt.find_node("/chosen").unwrap();
	find_kernel_linux(&chosen)
		.or_else(|| find_kernel_multiboot(&chosen))
		.expect("could not find kernel")
}

pub unsafe fn get_memory(memory_size: u64) -> u64 {
	let memory_size = usize::try_from(memory_size).unwrap();

	let initrd = AddressRange::try_from(find_kernel().as_ptr_range()).unwrap();
	let fdt = {
		let start = start::get_fdt_ptr();
		let end = unsafe { start.add(start::get_fdt().total_size()) };
		AddressRange::try_from(start..end).unwrap()
	};

	info!("initrd = {initrd}");
	info!("fdt    = {fdt}");

	const SUPERPAGE_SIZE: usize = 2 * 1024 * 1024;
	let initrd = initrd.align_to(SUPERPAGE_SIZE);
	let fdt = fdt.align_to(SUPERPAGE_SIZE);

	let [first, second] = if initrd < fdt {
		[initrd, fdt]
	} else {
		[fdt, initrd]
	};

	let start_address = if first.next(memory_size).overlaps(second) {
		second.end()
	} else {
		first.end()
	};

	u64::try_from(start_address).unwrap()
}

pub unsafe fn boot_kernel(kernel_info: LoadedKernel) -> ! {
	let LoadedKernel {
		load_info,
		entry_point,
	} = kernel_info;

	let fdt = start::get_fdt();

	let phys_addr_range = {
		let memory = fdt.memory();
		let mut regions = memory.regions();

		let mem_region = regions.next().unwrap();
		assert!(
			regions.next().is_none(),
			"hermit-loader can only handle one memory region yet"
		);

		let mem_base = u64::try_from(mem_region.starting_address.addr()).unwrap();
		let mem_size = u64::try_from(mem_region.size.unwrap()).unwrap();
		mem_base..mem_base + mem_size
	};

	let device_tree = {
		let fdt_addr = start::get_fdt_ptr().expose_addr();
		DeviceTreeAddress::new(fdt_addr.try_into().unwrap())
	};

	let boot_info = BootInfo {
		hardware_info: HardwareInfo {
			phys_addr_range,
			serial_port_base: None,
			device_tree,
		},
		load_info,
		platform_info: PlatformInfo::LinuxBoot,
	};

	let stack = start::get_stack_ptr();
	let entry = sptr::from_exposed_addr(entry_point.try_into().unwrap());
	let hart_id = start::get_hart_id();
	let raw_boot_info = boot_info.write();

	unsafe { enter_kernel(stack, entry, hart_id, raw_boot_info) }
}

unsafe fn enter_kernel(
	stack: *mut u8,
	entry: *const (),
	hart_id: usize,
	raw_boot_info: &'static RawBootInfo,
) -> ! {
	// Check expected signature of entry function
	let entry: Entry = {
		let entry: unsafe extern "C" fn(hart_id: usize, boot_info: &'static RawBootInfo) -> ! =
			unsafe { core::mem::transmute(entry) };
		entry
	};

	info!("Entering kernel at {entry:p}, stack at {stack:p}, raw_boot_info at {raw_boot_info:p}");

	unsafe {
		asm!(
			"mv sp, {stack}",
			"jr {entry}",
			entry = in(reg) entry,
			stack = in(reg) stack,
			in("a0") hart_id,
			in("a1") raw_boot_info,
			options(noreturn)
		)
	}
}
