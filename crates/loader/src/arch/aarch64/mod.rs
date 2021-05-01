pub mod bootinfo;
pub mod entry;
pub mod paging;
pub mod serial;

pub use crate::arch::bootinfo::*;
use crate::arch::paging::*;
use crate::arch::serial::SerialPort;
use goblin::elf;

extern "C" {
	static kernel_end: u8;
}

pub const ELF_ARCH: u16 = elf::header::EM_AARCH64;

const RAM_START: u64 = 0x40000000;
const SERIAL_PORT_ADDRESS: u32 = 0x9000000;

// VARIABLES
pub static mut BOOT_INFO: BootInfo = BootInfo::new();
static COM1: SerialPort = SerialPort::new(SERIAL_PORT_ADDRESS);

pub fn message_output_init() {
	// nothing to do
}

pub fn output_message_byte(byte: u8) {
	COM1.write_byte(byte);
}

pub unsafe fn get_memory(_memory_size: u64) -> u64 {
	align_up!(&kernel_end as *const u8 as u64, LargePageSize::SIZE as u64)
}

pub fn find_kernel() -> &'static [u8] {
	include_bytes!(env!("HERMIT_APP"))
}

pub unsafe fn boot_kernel(virtual_address: u64, mem_size: u64, entry_point: u64) -> ! {
	// Jump to the kernel entry point and provide the Multiboot information to it.
	loaderlog!(
		"Jumping to HermitCore Application Entry Point at 0x{:x}",
		entry_point
	);

	// Supply the parameters to the HermitCore application.
	BOOT_INFO.base = virtual_address;
	BOOT_INFO.limit = 0x40000000 /* start address of the RAM */ + 0x4000000 /* 64 MB */;
	BOOT_INFO.image_size = mem_size;
	BOOT_INFO.current_stack_address = RAM_START;
	//loaderlog!("BOOT_INFO:");
	//loaderlog!("==========");
	//loaderlog!("{:?}", BOOT_INFO);

	let func: extern "C" fn(boot_info: &'static mut BootInfo) -> ! =
		core::mem::transmute(entry_point);
	func(&mut BOOT_INFO)
}
