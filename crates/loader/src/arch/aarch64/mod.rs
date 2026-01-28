mod console;

pub use self::console::Console;
pub mod drivers;
pub mod entry;
pub mod paging;

use core::arch::asm;
use core::ptr;

use aarch64_cpu::asm::barrier::{NSH, SY, dmb, dsb, isb};
use align_address::Align;
use fdt::Fdt;
use goblin::elf::header::header64::{EI_DATA, ELFDATA2LSB, ELFMAG, Header, SELFMAG};
use hermit_entry::Entry;
use hermit_entry::boot_info::{BootInfo, HardwareInfo, PlatformInfo, RawBootInfo, SerialPortBase};
use hermit_entry::elf::LoadedKernel;
use log::info;

use crate::BootInfoExt;
use crate::arch::paging::*;
use crate::os::CONSOLE;

unsafe extern "C" {
	static mut loader_end: u8;
	static mut l0_pgtable: u64;
	static mut l1_pgtable: u64;
	static mut l2_pgtable: u64;
	static mut l2k_pgtable: u64;
	static mut l3_pgtable: u64;
	static mut L0mib_pgtable: u64;
	static mut fdt_addr: u64;
}

/// start address of the RAM at Qemu's virt emulation
const RAM_START: u64 = 0x40000000;
/// Default stack size of the kernel
const KERNEL_STACK_SIZE: usize = 32_768;
/// Qemu assumes for ELF kernel that the fdt is located at
/// start of RAM (0x4000_0000)
/// see <https://qemu.readthedocs.io/en/latest/system/arm/virt.html>
const DEVICE_TREE: u64 = RAM_START;

#[allow(dead_code)]
const PT_DEVICE: u64 = 0x707;
const PT_PT: u64 = 0x713;
const PT_MEM: u64 = 0x713;
const PT_MEM_CD: u64 = 0x70F;
const PT_SELF: u64 = 1 << 55;

pub unsafe fn get_memory(_memory_size: u64) -> u64 {
	(ptr::addr_of_mut!(loader_end).expose_provenance() as u64).align_up(LargePageSize::SIZE as u64)
}

pub unsafe fn get_fdt_addr() -> u64 {
	unsafe { if fdt_addr != 0 { fdt_addr } else { DEVICE_TREE } }
}

pub fn find_kernel() -> &'static [u8] {
	let fdt = unsafe {
		Fdt::from_ptr(ptr::with_exposed_provenance(get_fdt_addr() as usize))
			.expect(".fdt file has invalid header")
	};
	let module_start = fdt
		.find_node("/chosen")
		.unwrap()
		.children()
		.find(|node| node.name.starts_with("module@"))
		.map(|node| {
			let value = node.name.strip_prefix("module@").unwrap();
			if let Some(value) = value.strip_prefix("0x") {
				usize::from_str_radix(value, 16).unwrap()
			} else if let Some(value) = value.strip_prefix("0X") {
				usize::from_str_radix(value, 16).unwrap()
			} else {
				value.parse().unwrap()
			}
		})
		.unwrap();

	let header = unsafe {
		&*core::mem::transmute::<*const u8, *const Header>(ptr::with_exposed_provenance(
			module_start,
		))
	};

	if header.e_ident[0..SELFMAG] != ELFMAG[..] {
		panic!("Didn't find valid ELF file!");
	}

	#[cfg(target_endian = "little")]
	let file_size = if header.e_ident[EI_DATA] == ELFDATA2LSB {
		header.e_shoff + (header.e_shentsize as u64 * header.e_shnum as u64)
	} else {
		header.e_shoff.to_le() + (header.e_shentsize.to_le() as u64 * header.e_shnum.to_le() as u64)
	};
	#[cfg(target_endian = "big")]
	let file_size = if header.e_ident[EI_DATA] == ELFDATA2LSB {
		header.e_shoff.to_be() + (header.e_shentsize.to_be() as u64 * header.e_shnum.to_be() as u64)
	} else {
		header.e_shoff + (header.e_shentsize as u64 * header.e_shnum as u64)
	};

	info!("Found ELF file with size {file_size}");

	unsafe {
		core::slice::from_raw_parts(
			ptr::with_exposed_provenance(module_start),
			file_size.try_into().unwrap(),
		)
	}
}

pub unsafe fn boot_kernel(kernel_info: LoadedKernel) -> ! {
	let LoadedKernel {
		load_info,
		entry_point,
	} = kernel_info;

	let fdt = unsafe {
		Fdt::from_ptr(ptr::with_exposed_provenance(get_fdt_addr() as usize))
			.expect(".fdt file has invalid header")
	};
	let cpus = fdt.cpus().count();
	info!("Detect {cpus} CPU(s)");

	let uart_address: u32 = CONSOLE.lock().get().get_stdout();
	info!("Detect UART at {uart_address:#x}");

	let pgt_slice = unsafe { core::slice::from_raw_parts_mut(ptr::addr_of_mut!(l0_pgtable), 512) };
	for i in pgt_slice.iter_mut() {
		*i = 0;
	}
	pgt_slice[0] = ptr::addr_of_mut!(l1_pgtable).expose_provenance() as u64 + PT_PT;
	pgt_slice[511] = ptr::addr_of_mut!(l0_pgtable).expose_provenance() as u64 + PT_PT + PT_SELF;

	let pgt_slice = unsafe { core::slice::from_raw_parts_mut(ptr::addr_of_mut!(l1_pgtable), 512) };
	for i in pgt_slice.iter_mut() {
		*i = 0;
	}
	pgt_slice[0] = ptr::addr_of_mut!(l2_pgtable).expose_provenance() as u64 + PT_PT;
	pgt_slice[1] = ptr::addr_of_mut!(l2k_pgtable).expose_provenance() as u64 + PT_PT;

	let pgt_slice = unsafe { core::slice::from_raw_parts_mut(ptr::addr_of_mut!(l2_pgtable), 512) };
	for i in pgt_slice.iter_mut() {
		*i = 0;
	}
	pgt_slice[0] = ptr::addr_of_mut!(l3_pgtable).expose_provenance() as u64 + PT_PT;

	let pgt_slice = unsafe { core::slice::from_raw_parts_mut(ptr::addr_of_mut!(l3_pgtable), 512) };
	for i in pgt_slice.iter_mut() {
		*i = 0;
	}
	pgt_slice[1] = uart_address as u64 + PT_MEM_CD;

	// map kernel to loader_start and stack below the kernel
	let pgt_slice = unsafe { core::slice::from_raw_parts_mut(ptr::addr_of_mut!(l2k_pgtable), 512) };
	for i in pgt_slice.iter_mut() {
		*i = 0;
	}
	for (i, pgt_slice) in pgt_slice.iter_mut().enumerate().take(10) {
		*pgt_slice = ptr::addr_of_mut!(L0mib_pgtable).expose_provenance() as u64
			+ (i * BasePageSize::SIZE) as u64
			+ PT_PT;
	}

	let pgt_slice =
		unsafe { core::slice::from_raw_parts_mut(ptr::addr_of_mut!(L0mib_pgtable), 10 * 512) };
	for (i, entry) in pgt_slice.iter_mut().enumerate() {
		*entry = RAM_START + (i * BasePageSize::SIZE) as u64 + PT_MEM;
	}

	CONSOLE.lock().get().set_stdout(0x1000);

	// Load TTBRx
	unsafe {
		asm!(
				"msr ttbr1_el1, xzr",
				"msr ttbr0_el1, {}",
				"dsb sy",
				"isb",
				in(reg) ptr::addr_of_mut!(l0_pgtable),
				options(nostack),
		)
	};

	// Enable paging
	unsafe {
		asm!(
				"mrs x0, sctlr_el1",
				"orr x0, x0, #1",
				"msr sctlr_el1, x0",
				"bl 0f",
				"0:",
				out("x0") _,
				options(nostack),
		);
	}

	info!("Successfully set up paging.");

	let fdt = unsafe {
		Fdt::from_ptr(ptr::with_exposed_provenance(DEVICE_TREE as usize))
			.expect(".fdt file has invalid header")
	};

	if let Some(device_type) = fdt
		.find_node("/memory")
		.and_then(|node| node.property("device_type"))
	{
		let device_type = core::str::from_utf8(device_type.value)
			.unwrap()
			.trim_matches(char::from(0));
		assert!(device_type == "memory");
	}
	info!("Memory found!");
	let regions = fdt.memory().regions().next().unwrap();
	let ram_start = regions.starting_address as u64;
	let ram_size = regions.size.unwrap() as u64;

	info!("ram_start: {ram_start:#x}, ram_size: {ram_size:#x}. Trying to jump into kernel soon.");
	let boot_info = BootInfo {
		hardware_info: HardwareInfo {
			phys_addr_range: ram_start..ram_start + ram_size,
			serial_port_base: SerialPortBase::new(0x1000),
			device_tree: unsafe { core::num::NonZeroU64::new(get_fdt_addr()) },
		},
		load_info,
		platform_info: PlatformInfo::LinuxBoot,
	};

	let stack = boot_info.load_info.kernel_image_addr_range.start as usize - KERNEL_STACK_SIZE;
	let stack = ptr::with_exposed_provenance_mut(stack);
	let entry = ptr::with_exposed_provenance(entry_point.try_into().unwrap());
	let raw_boot_info = boot_info.write();

	unsafe { enter_kernel(stack, entry, raw_boot_info) }
}

unsafe fn enter_kernel(stack: *mut u8, entry: *const (), raw_boot_info: &'static RawBootInfo) -> ! {
	// Check expected signature of entry function
	let entry: Entry = {
		let entry: unsafe extern "C" fn(raw_boot_info: &'static RawBootInfo, cpu_id: u32) -> ! =
			unsafe { core::mem::transmute(entry) };
		entry
	};

	info!("Entering kernel at {entry:p}, stack at {stack:p}, raw_boot_info at {raw_boot_info:p}");

	// Memory barrier
	CONSOLE.lock().get().wait_empty();
	dsb(SY);
	isb(SY);
	dmb(SY);
	dsb(NSH);

	unsafe {
		asm!(
			"mov sp, {stack}",
			"br {entry}",
			stack = in(reg) stack,
			entry = in(reg) entry,
			in("x0") raw_boot_info,
			in("x1") 0,
			options(noreturn)
		)
	}
}
