mod gdt;
mod page_tables;
mod stack;

use core::sync::atomic::{AtomicU32, Ordering};

use hermit_entry::boot_info::{BootInfo, HardwareInfo, LoadInfo, PlatformInfo, SerialPortBase};
use pvh::start_info::reader::{IdentityMap, StartInfoReader};

use self::stack::{STACK, Stack};
use crate::env::setboot_info2;
use crate::kernel::pre_init;

/// The PVH entry point.
#[unsafe(naked)]
pub(crate) unsafe extern "C" fn pvh_start32() -> ! {
	core::arch::naked_asm!(
		".code32",
		include_str!("pvh_start32.s"),
		".code64",

		level_4_table = sym page_tables::LEVEL_4_TABLE,
		gdt_ptr = sym gdt::GDT_PTR,
		kernel_data_selector = const gdt::Gdt::kernel_data_selector().0,

		stack = sym STACK,
		stack_size = const size_of::<Stack>(),
		kernel_code_selector = const gdt::Gdt::kernel_code_selector().0,
		rust_start = sym rust_start,
	);
}

pvh::xen_elfnote_phys32_entry!(pvh_start32);

/// The native ELF entry point.
#[unsafe(no_mangle)]
#[unsafe(naked)]
unsafe extern "C" fn _start() -> ! {
	core::arch::naked_asm!("2: jmp 2b");
}

static START_INFO_PADDR: AtomicU32 = AtomicU32::new(0);

pub fn start_info<'a>() -> StartInfoReader<'a, IdentityMap> {
	let paddr = START_INFO_PADDR.load(Ordering::Relaxed);
	unsafe { StartInfoReader::from_paddr_identity(paddr).unwrap() }
}

/// The Rust entry point.
unsafe extern "C" fn rust_start(start_info_paddr: u32) -> ! {
	START_INFO_PADDR.store(start_info_paddr, Ordering::Relaxed);

	let start_info = start_info();

	println!("Start info:\n{start_info:#?}");

	let boot_info = BootInfo {
		hardware_info: HardwareInfo {
			phys_addr_range: 0..0,
			serial_port_base: SerialPortBase::new(0x3f8),
			device_tree: None,
		},
		// This is not used by the kernel anymore.
		load_info: LoadInfo {
			kernel_image_addr_range: 0..0,
			tls_info: None,
		},
		platform_info: PlatformInfo::Fdt,
	};

	println!("boot_info = {boot_info:#x?}");

	setboot_info2(boot_info);

	unsafe { pre_init(None, 0) }
}
