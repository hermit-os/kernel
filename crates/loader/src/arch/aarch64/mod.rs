pub mod entry;
pub mod paging;
pub mod serial;

use core::arch::asm;

use goblin::elf::header::header64::{Header, EI_DATA, ELFDATA2LSB, ELFMAG, SELFMAG};
use hermit_entry::{
	boot_info::{BootInfo, HardwareInfo, PlatformInfo, RawBootInfo, SerialPortBase},
	elf::LoadedKernel,
	Entry,
};
use log::info;

use crate::arch::paging::*;
use crate::arch::serial::SerialPort;

extern "C" {
	static kernel_end: u8;
	static mut l0_pgtable: u64;
	static mut l1_pgtable: u64;
	static mut l2_pgtable: u64;
	static mut l2k_pgtable: u64;
	static mut l3_pgtable: u64;
	static mut L0mib_pgtable: u64;
}

/// start address of the RAM at Qemu's virt emulation
const RAM_START: u64 = 0x40000000;
/// Physical address of UART0 at Qemu's virt emulation
const SERIAL_PORT_ADDRESS: u32 = 0x09000000;
/// Default stack size of the kernel
const KERNEL_STACK_SIZE: usize = 32_768;
/// Qemu assumes for ELF kernel that the DTB is located at
/// start of RAM (0x4000_0000)
/// see <https://qemu.readthedocs.io/en/latest/system/arm/virt.html>
const FDT: u64 = RAM_START;

#[allow(dead_code)]
const PT_DEVICE: u64 = 0x707;
const PT_PT: u64 = 0x713;
const PT_MEM: u64 = 0x713;
const PT_MEM_CD: u64 = 0x70F;
const PT_SELF: u64 = 1 << 55;

// VARIABLES
static mut COM1: SerialPort = SerialPort::new(SERIAL_PORT_ADDRESS);

pub fn message_output_init() {
	let fdt = unsafe {
		core::slice::from_raw_parts(
			FDT as *mut u8,
			&kernel_end as *const u8 as usize - RAM_START as usize,
		)
	};
	let fdt = fdt::Fdt::new(fdt).unwrap();

	let uart_address: u32 = if let Some(stdout) = fdt.chosen().stdout() {
		if let Some(pos) = stdout.name.find("@") {
			let len = stdout.name.len();
			u32::from_str_radix(&stdout.name[pos + 1..len], 16).unwrap_or(SERIAL_PORT_ADDRESS)
		} else {
			SERIAL_PORT_ADDRESS
		}
	} else {
		SERIAL_PORT_ADDRESS
	};

	unsafe {
		COM1.set_port(uart_address);
	}
}

pub fn output_message_byte(byte: u8) {
	unsafe {
		COM1.write_byte(byte);
	}
}

pub unsafe fn get_memory(_memory_size: u64) -> u64 {
	align_up!(&kernel_end as *const u8 as u64, LargePageSize::SIZE as u64)
}

fn print_node(node: fdt::node::FdtNode<'_, '_>, n_spaces: usize) {
    (0..n_spaces).for_each(|_| print!(" "));
    println!("{}/", node.name);

    for child in node.children() {
        print_node(child, n_spaces + 4);
    }
}

pub fn find_kernel() -> &'static [u8] {
	let fdt = unsafe {
		core::slice::from_raw_parts(
			FDT as *mut u8,
			&kernel_end as *const u8 as usize - RAM_START as usize,
		)
	};
	let fdt = fdt::Fdt::new(fdt).unwrap();

	print_node(fdt.find_node("/").unwrap(), 0);

	let chosen = fdt.find_node("/chosen").unwrap();
	let module_start = chosen
		.children()
		.find(|node| node.name.starts_with("module@"))
		.map(|node| {
			let value = node.name.strip_prefix("module@").unwrap();
			if let Some(value) = value.strip_prefix("0x") {
				info!("value {}", value);
				usize::from_str_radix(value, 16).unwrap()
			} else if let Some(value) = value.strip_prefix("0X") {
				usize::from_str_radix(value, 16).unwrap()
			} else {
				usize::from_str_radix(value, 10).unwrap()
			}
		})
		.unwrap();
	let header =
		unsafe { &*core::mem::transmute::<*const u8, *const Header>(module_start as *const u8) };

	for i in 0..SELFMAG {
		if header.e_ident[i] != ELFMAG[i] {
			panic!("Don't found valid ELF file!");
		}
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

	info!("Found ELF file with size {}", file_size);

	unsafe { core::slice::from_raw_parts(module_start as *const u8, file_size.try_into().unwrap()) }
}

pub unsafe fn boot_kernel(kernel_info: LoadedKernel) -> ! {
	let LoadedKernel {
		load_info,
		entry_point,
	} = kernel_info;

	let fdt = unsafe {
		core::slice::from_raw_parts(
			FDT as *mut u8,
			&kernel_end as *const u8 as usize - RAM_START as usize,
		)
	};
	let fdt = fdt::Fdt::new(fdt).unwrap();
	info!("Detect {} CPU(s)", fdt.cpus().count());

	let uart_address: u32 = unsafe { COM1.get_port() };
	info!("Detect UART at {:#x}", uart_address);

	let pgt_slice = core::slice::from_raw_parts_mut(&mut l0_pgtable as *mut u64, 512);
	for i in pgt_slice.iter_mut() {
		*i = 0;
	}
	pgt_slice[0] = &l1_pgtable as *const u64 as u64 + PT_PT;
	pgt_slice[511] = &l0_pgtable as *const u64 as u64 + PT_PT + PT_SELF;

	let pgt_slice = core::slice::from_raw_parts_mut(&mut l1_pgtable as *mut u64, 512);
	for i in pgt_slice.iter_mut() {
		*i = 0;
	}
	pgt_slice[0] = &l2_pgtable as *const _ as u64 + PT_PT;
	pgt_slice[1] = &l2k_pgtable as *const _ as u64 + PT_PT;

	let pgt_slice = core::slice::from_raw_parts_mut(&mut l2_pgtable as *mut u64, 512);
	for i in pgt_slice.iter_mut() {
		*i = 0;
	}
	pgt_slice[0] = &l3_pgtable as *const u64 as u64 + PT_PT;

	let pgt_slice = core::slice::from_raw_parts_mut(&mut l3_pgtable as *mut u64, 512);
	for i in pgt_slice.iter_mut() {
		*i = 0;
	}
	pgt_slice[1] = uart_address as u64 + PT_MEM_CD;

	// map kernel to KERNEL_START and stack below the kernel
	let pgt_slice = core::slice::from_raw_parts_mut(&mut l2k_pgtable as *mut u64, 512);
	for i in pgt_slice.iter_mut() {
		*i = 0;
	}
	for i in 0..10 {
		pgt_slice[i] =
			&mut L0mib_pgtable as *mut _ as u64 + (i * BasePageSize::SIZE) as u64 + PT_PT;
	}

	let pgt_slice = core::slice::from_raw_parts_mut(&mut L0mib_pgtable as *mut u64, 10 * 512);
	for (i, entry) in pgt_slice.iter_mut().enumerate() {
		*entry = RAM_START + (i * BasePageSize::SIZE) as u64 + PT_MEM;
	}

	COM1.set_port(0x1000);

	// Load TTBRx
	asm!(
			"msr ttbr1_el1, xzr",
			"msr ttbr0_el1, {}",
			"dsb sy",
			"isb",
			in(reg) &l0_pgtable as *const _ as u64,
			options(nostack),
	);

	// Enable paging
	asm!(
			"mrs x0, sctlr_el1",
			"orr x0, x0, #1",
			"msr sctlr_el1, x0",
			"bl 0f",
			"0:",
			out("x0") _,
			options(nostack),
	);

	let current_stack_address = load_info.kernel_image_addr_range.start - KERNEL_STACK_SIZE as u64;
	pub static mut BOOT_INFO: Option<RawBootInfo> = None;

	let ram_start = fdt.memory().regions().next().unwrap().starting_address as u64;
	let ram_size = fdt
		.memory()
		.regions()
		.next()
		.unwrap()
		.size
		.unwrap_or(0x20000000usize) as u64;

	BOOT_INFO = {
		let boot_info = BootInfo {
			hardware_info: HardwareInfo {
				phys_addr_range: ram_start..ram_start + ram_size,
				serial_port_base: SerialPortBase::new(0x1000),
                device_tree: core::num::NonZeroU64::new(FDT),
			},
			load_info,
			platform_info: PlatformInfo::LinuxBoot,
		};
		Some(RawBootInfo::from(boot_info))
	};

	// Jump to the kernel entry point and provide the Multiboot information to it.
	info!(
		"Jumping to HermitCore Application Entry Point at {:#x}",
		entry_point
	);

	/* Memory barrier */
	asm!("dsb sy", options(nostack));

	#[allow(dead_code)]
	const ENTRY_TYPE_CHECK: Entry = {
		unsafe extern "C" fn entry_signature(
			_raw_boot_info: &'static RawBootInfo,
			_cpu_id: u32,
		) -> ! {
			unimplemented!()
		}
		entry_signature
	};

	asm!(
		"mov sp, {stack_address}",
		"br {entry}",
		stack_address = in(reg) current_stack_address,
		entry = in(reg) entry_point,
		in("x0") BOOT_INFO.as_ref().unwrap(),
		in("x1") 0,
		options(noreturn)
	);
}
