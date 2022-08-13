pub mod irq;
pub mod pci;
pub mod percore;
pub mod processor;
pub mod scheduler;
pub mod serial;
mod start;
pub mod stubs;
pub mod switch;
pub mod systemtime;

pub use crate::arch::aarch64::kernel::stubs::*;
pub use crate::arch::aarch64::kernel::systemtime::get_boot_time;

use core::arch::{asm, global_asm};
use core::ptr;

use hermit_entry::boot_info::{BootInfo, PlatformInfo, RawBootInfo};

use crate::arch::aarch64::kernel::percore::*;
use crate::arch::aarch64::kernel::serial::SerialPort;
use crate::arch::aarch64::mm::{PhysAddr, VirtAddr};
use crate::config::*;
use crate::env;
use crate::synch::spinlock::Spinlock;

const SERIAL_PORT_BAUDRATE: u32 = 115200;

static mut COM1: SerialPort = SerialPort::new(0x800);
static CPU_ONLINE: Spinlock<u32> = Spinlock::new(0);

global_asm!(include_str!("start.s"));

/// Kernel header to announce machine features
#[cfg_attr(
	all(target_os = "none", not(feature = "newlib")),
	link_section = ".data"
)]
#[cfg_attr(all(target_os = "none", feature = "newlib"), link_section = ".mboot")]
static mut RAW_BOOT_INFO: Option<&'static RawBootInfo> = None;
static mut BOOT_INFO: Option<BootInfo> = None;

pub fn boot_info() -> &'static BootInfo {
	unsafe { BOOT_INFO.as_ref().unwrap() }
}

pub fn raw_boot_info() -> &'static RawBootInfo {
	unsafe { RAW_BOOT_INFO.unwrap() }
}

pub fn get_boot_info_address() -> VirtAddr {
	VirtAddr(raw_boot_info() as *const _ as u64)
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

pub fn get_tls_start() -> VirtAddr {
	VirtAddr(
		boot_info()
			.load_info
			.tls_info
			.as_ref()
			.map(|tls_info| tls_info.start)
			.unwrap_or_default(),
	)
}

pub fn get_tls_filesz() -> usize {
	boot_info()
		.load_info
		.tls_info
		.as_ref()
		.map(|tls_info| tls_info.filesz)
		.unwrap_or_default() as usize
}

pub fn get_tls_memsz() -> usize {
	boot_info()
		.load_info
		.tls_info
		.as_ref()
		.map(|tls_info| tls_info.memsz)
		.unwrap_or_default() as usize
}

pub fn get_tls_align() -> usize {
	boot_info()
		.load_info
		.tls_info
		.as_ref()
		.map(|tls_info| tls_info.align)
		.unwrap_or_default() as usize
}

#[cfg(feature = "smp")]
pub fn get_possible_cpus() -> u32 {
	todo!()
}

#[cfg(feature = "smp")]
pub fn get_processor_count() -> u32 {
	todo!()
}

#[cfg(not(feature = "smp"))]
pub fn get_processor_count() -> u32 {
	1
}

/// Whether HermitCore is running under the "uhyve" hypervisor.
pub fn is_uhyve() -> bool {
	matches!(boot_info().platform_info, PlatformInfo::Uhyve { .. })
}

pub fn get_cmdsize() -> usize {
	todo!()
}

pub fn get_cmdline() -> VirtAddr {
	todo!()
}

/// Earliest initialization function called by the Boot Processor.
pub fn message_output_init() {
	percore::init();

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
pub fn boot_processor_init() {
	//processor::configure();

	crate::mm::init();
	crate::mm::print_information();

	return;
	/*processor::detect_features();
	processor::configure();

	::mm::init();
	::mm::print_information();
	env::init();
	gdt::init();
	gdt::add_current_core();
	idt::install();

	if !env::is_uhyve() {
		pic::init();
	}

	irq::install();
	irq::enable();
	processor::detect_frequency();
	processor::print_information();
	systemtime::init();

	if !env::is_uhyve() {
		pci::init();
		pci::print_information();
		acpi::init();
	}

	apic::init();
	scheduler::install_timer_handler();*/

	// Read out PMCCNTR_EL0 in an infinite loop.
	// TODO: This currently stays at zero on uhyve. Fix uhyve! :)
	loop {
		unsafe {
			let pmccntr: u64;
			asm!("mrs {}, pmccntr_el0", out(reg) pmccntr, options(nomem, nostack));
			println!("Count: {}", pmccntr);
		}
	}

	finish_processor_init();
}

/// Boots all available Application Processors on bare-metal or QEMU.
/// Called after the Boot Processor has been fully initialized along with its scheduler.
pub fn boot_application_processors() {
	// Nothing to do here yet.
}

/// Application Processor initialization
pub fn application_processor_init() {
	percore::init();
	/*processor::configure();
	gdt::add_current_core();
	idt::install();
	apic::init_x2apic();
	apic::init_local_apic();
	irq::enable();*/
	finish_processor_init();
}

fn finish_processor_init() {
	debug!("Initialized Processor");

	/*if env::is_uhyve() {
		// uhyve does not use apic::detect_from_acpi and therefore does not know the number of processors and
		// their APIC IDs in advance.
		// Therefore, we have to add each booted processor into the CPU_LOCAL_APIC_IDS vector ourselves.
		// Fortunately, the Core IDs are guaranteed to be sequential and match the Local APIC IDs.
		apic::add_local_apic_id(core_id() as u8);

		// uhyve also boots each processor into _start itself and does not use apic::boot_application_processors.
		// Therefore, the current processor already needs to prepare the processor variables for a possible next processor.
		apic::init_next_processor_variables(core_id() + 1);
	}*/

	// This triggers apic::boot_application_processors (bare-metal/QEMU) or uhyve
	// to initialize the next processor.
	*CPU_ONLINE.lock() += 1;
}

pub fn network_adapter_init() -> i32 {
	// AArch64 supports no network adapters on bare-metal/QEMU, so return a failure code.
	-1
}

pub fn print_statistics() {}
