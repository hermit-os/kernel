pub mod core_local;
pub mod interrupts;
#[cfg(all(not(feature = "pci"), any(feature = "tcp", feature = "udp")))]
pub mod mmio;
#[cfg(feature = "pci")]
pub mod pci;
pub mod processor;
pub mod scheduler;
pub mod serial;
#[cfg(target_os = "none")]
mod start;
pub mod switch;
pub mod systemtime;

use core::arch::global_asm;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use core::{ptr, str};

use hermit_entry::boot_info::{BootInfo, RawBootInfo};

use crate::arch::aarch64::kernel::core_local::*;
use crate::arch::aarch64::kernel::serial::SerialPort;
use crate::arch::aarch64::mm::{PhysAddr, VirtAddr};
use crate::env;

global_asm!(include_str!("setjmp.s"));
global_asm!(include_str!("longjmp.s"));

const SERIAL_PORT_BAUDRATE: u32 = 115200;

static mut COM1: SerialPort = SerialPort::new(0x800);

/// `CPU_ONLINE` is the count of CPUs that finished initialization.
///
/// It also synchronizes initialization of CPU cores.
pub(crate) static CPU_ONLINE: AtomicU32 = AtomicU32::new(0);

pub(crate) static CURRENT_STACK_ADDRESS: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "none")]
global_asm!(include_str!("start.s"));

/// Kernel header to announce machine features
#[cfg_attr(target_os = "none", link_section = ".data")]
static mut RAW_BOOT_INFO: Option<&'static RawBootInfo> = None;
static mut BOOT_INFO: Option<BootInfo> = None;

pub fn boot_info() -> &'static BootInfo {
	unsafe { BOOT_INFO.as_ref().unwrap() }
}

pub fn raw_boot_info() -> &'static RawBootInfo {
	unsafe { RAW_BOOT_INFO.unwrap() }
}

pub fn get_boot_info_address() -> VirtAddr {
	VirtAddr(ptr::from_ref(raw_boot_info()).addr() as u64)
}

pub fn is_uhyve_with_pci() -> bool {
	false
}

pub fn get_ram_address() -> PhysAddr {
	PhysAddr(boot_info().hardware_info.phys_addr_range.start)
}

pub fn get_base_address() -> VirtAddr {
	VirtAddr(boot_info().load_info.kernel_image_addr_range.start)
}

pub fn get_image_size() -> usize {
	let range = &boot_info().load_info.kernel_image_addr_range;
	(range.end - range.start) as usize
}

pub fn get_limit() -> usize {
	boot_info().hardware_info.phys_addr_range.end as usize
}

#[cfg(feature = "smp")]
pub fn get_possible_cpus() -> u32 {
	CPU_ONLINE.load(Ordering::Acquire)
}

#[cfg(feature = "smp")]
pub fn get_processor_count() -> u32 {
	1
}

#[cfg(not(feature = "smp"))]
pub fn get_processor_count() -> u32 {
	1
}

pub fn args() -> Option<&'static str> {
	let dtb = unsafe {
		hermit_dtb::Dtb::from_raw(ptr::with_exposed_provenance(
			boot_info().hardware_info.device_tree.unwrap().get() as usize,
		))
		.expect(".dtb file has invalid header")
	};

	dtb.get_property("/chosen", "bootargs")
		.map(|property| str::from_utf8(property).unwrap())
}

/// Earliest initialization function called by the Boot Processor.
pub fn message_output_init() {
	CoreLocal::install();

	unsafe {
		COM1.port_address = boot_info()
			.hardware_info
			.serial_port_base
			.map(|uartport| uartport.get())
			.unwrap_or_default()
			.try_into()
			.unwrap();
	}

	// We can only initialize the serial port here, because VGA requires processor
	// configuration first.
	unsafe {
		COM1.init(SERIAL_PORT_BAUDRATE);
	}
}

pub fn output_message_byte(byte: u8) {
	// Output messages to the serial port.
	unsafe {
		COM1.write_byte(byte);
	}
}

pub fn output_message_buf(buf: &[u8]) {
	for byte in buf {
		output_message_byte(*byte);
	}
}

/// Real Boot Processor initialization as soon as we have put the first Welcome message on the screen.
#[cfg(target_os = "none")]
pub fn boot_processor_init() {
	processor::configure();

	crate::mm::init();
	crate::mm::print_information();
	CoreLocal::get().add_irq_counter();
	env::init();
	interrupts::init();
	interrupts::enable();
	processor::detect_frequency();
	processor::print_information();
	systemtime::init();
	#[cfg(feature = "pci")]
	pci::init();

	finish_processor_init();
}

/// Boots all available Application Processors on bare-metal or QEMU.
/// Called after the Boot Processor has been fully initialized along with its scheduler.
pub fn boot_application_processors() {
	// Nothing to do here yet.
}

/// Application Processor initialization
#[allow(dead_code)]
pub fn application_processor_init() {
	CoreLocal::install();
	finish_processor_init();
}

fn finish_processor_init() {
	debug!("Initialized Processor");

	CPU_ONLINE.fetch_add(1, Ordering::Release);
}

pub fn print_statistics() {
	interrupts::print_statistics();
}
