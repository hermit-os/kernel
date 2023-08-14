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

pub fn message_output_init() {}

pub use sbi::legacy::console_putchar as output_message_byte;

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
		AddressRange::try_from(start..start.add(start::get_fdt().total_size())).unwrap()
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

	info!("hart_id = {}", start::get_hart_id());

	static mut BOOT_INFO: Option<RawBootInfo> = None;

	BOOT_INFO = {
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

		info!("boot_info = {boot_info:#?}");

		Some(RawBootInfo::from(boot_info))
	};

	// Check expected signature of entry function
	let entry: Entry = {
		let entry: unsafe extern "C" fn(hart_id: usize, boot_info: &'static RawBootInfo) -> ! =
			core::mem::transmute(entry_point);
		entry
	};

	info!("Jumping into kernel at {entry:p}");

	asm!(
		"mv sp, {stack}",
		"jr {entry}",
		entry = in(reg) entry,
		stack = in(reg) start::get_stack_ptr(),
		in("a0") start::get_hart_id(),
		in("a1") BOOT_INFO.as_ref().unwrap(),
		options(noreturn)
	)
}
