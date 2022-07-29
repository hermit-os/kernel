pub mod entry;
pub mod paging;
pub mod serial;

use core::arch::asm;

use hermit_entry::{BootInfo, Entry, PlatformInfo, RawBootInfo, SerialPortBase, TlsInfo};

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

pub const ELF_ARCH: u16 = goblin::elf::header::EM_AARCH64;
pub const R_RELATIVE: u32 = goblin::elf::reloc::R_AARCH64_RELATIVE;

/// start address of the RAM at Qemu's virt emulation
const RAM_START: u64 = 0x40000000;
/// Physical address of UART0 at Qemu's virt emulation
const SERIAL_PORT_ADDRESS: u32 = 0x09000000;
/// Default stack size of the kernel
const KERNEL_STACK_SIZE: usize = 32_768;

const PT_DEVICE: u64 = 0x707;
const PT_PT: u64 = 0x713;
const PT_MEM: u64 = 0x713;
const PT_MEM_CD: u64 = 0x70F;
const PT_SELF: u64 = 1 << 55;

// VARIABLES
static mut COM1: SerialPort = SerialPort::new(SERIAL_PORT_ADDRESS);

pub fn message_output_init() {
	// nothing to do
}

pub fn output_message_byte(byte: u8) {
	unsafe {
		COM1.write_byte(byte);
	}
}

pub unsafe fn get_memory(_memory_size: u64) -> u64 {
	align_up!(&kernel_end as *const u8 as u64, LargePageSize::SIZE as u64)
}

pub fn find_kernel() -> &'static [u8] {
	align_data::include_aligned!(goblin::elf64::header::Header, env!("HERMIT_APP"))
}

pub unsafe fn boot_kernel(
	tls_info: Option<TlsInfo>,
	_elf_address: Option<u64>,
	virtual_address: u64,
	mem_size: u64,
	entry_point: u64,
) -> ! {
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
	pgt_slice[1] = SERIAL_PORT_ADDRESS as u64 + PT_MEM_CD;

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

	let func: Entry = core::mem::transmute(entry_point);
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

	pub static mut BOOT_INFO: RawBootInfo = RawBootInfo::invalid();
	BOOT_INFO = {
		let boot_info = BootInfo {
			phys_addr_range: RAM_START..RAM_START + 0x20000000, // 512 MB
			kernel_image_addr_range: virtual_address..virtual_address + mem_size,
			tls_info,
			serial_port_base: SerialPortBase::new(0x1000),
			platform_info: PlatformInfo::LinuxBoot,
		};
		let raw_boot_info = RawBootInfo::from(boot_info);
		raw_boot_info.store_current_stack_address(virtual_address - KERNEL_STACK_SIZE as u64);
		raw_boot_info
	};

	// Jump to the kernel entry point and provide the Multiboot information to it.
	loaderlog!(
		"Jumping to HermitCore Application Entry Point at {:#x}",
		entry_point
	);

	/* Memory barrier */
	asm!("dsb sy", options(nostack));

	func(&BOOT_INFO)
}
