mod bootinfo;
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

use crate::arch::aarch64::kernel::bootinfo::BootInfo;
use crate::arch::aarch64::kernel::percore::*;
use crate::arch::aarch64::kernel::serial::SerialPort;
pub use crate::arch::aarch64::kernel::stubs::*;
pub use crate::arch::aarch64::kernel::systemtime::get_boot_time;
use crate::arch::aarch64::mm::{PhysAddr, VirtAddr};
use crate::config::*;
use crate::env;
use crate::kernel_message_buffer;
use crate::synch::spinlock::Spinlock;
use core::arch::{asm, global_asm};
use core::ptr;

const SERIAL_PORT_BAUDRATE: u32 = 115200;

static mut COM1: SerialPort = SerialPort::new(0x800);
static CPU_ONLINE: Spinlock<u32> = Spinlock::new(0);

/// Kernel header to announce machine features
#[cfg(not(target_os = "none"))]
static mut BOOT_INFO: *mut BootInfo = ptr::null_mut();

#[cfg(all(target_os = "none", not(feature = "newlib")))]
#[link_section = ".data"]
static mut BOOT_INFO: *mut BootInfo = ptr::null_mut();

#[cfg(all(target_os = "none", feature = "newlib"))]
#[link_section = ".mboot"]
static mut BOOT_INFO: *mut BootInfo = ptr::null_mut();

// FUNCTIONS

global_asm!(include_str!("start.s"));

pub fn get_boot_info_address() -> VirtAddr {
	VirtAddr(unsafe { BOOT_INFO as u64 })
}

pub fn get_ram_address() -> PhysAddr {
	unsafe { PhysAddr(core::ptr::read_volatile(&(*BOOT_INFO).ram_start)) }
}

pub fn get_image_size() -> usize {
	unsafe { core::ptr::read_volatile(&(*BOOT_INFO).image_size) as usize }
}

pub fn get_limit() -> usize {
	unsafe { core::ptr::read_volatile(&(*BOOT_INFO).limit) as usize }
}

pub fn get_processor_count() -> u32 {
	unsafe { core::ptr::read_volatile(&(*BOOT_INFO).cpu_online) }
}

pub fn get_base_address() -> VirtAddr {
	unsafe { VirtAddr(core::ptr::read_volatile(&(*BOOT_INFO).base)) }
}

pub fn get_tls_start() -> VirtAddr {
	unsafe { VirtAddr(core::ptr::read_volatile(&(*BOOT_INFO).tls_start)) }
}

pub fn get_current_stack_address() -> VirtAddr {
	unsafe {
		VirtAddr(core::ptr::read_volatile(
			&(*BOOT_INFO).current_stack_address,
		))
	}
}

pub fn get_tls_filesz() -> usize {
	unsafe { core::ptr::read_volatile(&(*BOOT_INFO).tls_filesz) as usize }
}

pub fn get_tls_memsz() -> usize {
	unsafe { core::ptr::read_volatile(&(*BOOT_INFO).tls_memsz) as usize }
}

pub fn get_tls_align() -> usize {
	unsafe { core::ptr::read_volatile(&(*BOOT_INFO).tls_align) as usize }
}

/// Whether HermitCore is running under the "uhyve" hypervisor.
pub fn is_uhyve() -> bool {
	unsafe { core::ptr::read_volatile(&(*BOOT_INFO).uhyve) != 0 }
}

/// Whether HermitCore is running alone (true) or side-by-side to Linux in Multi-Kernel mode (false).
pub fn is_single_kernel() -> bool {
	unsafe { core::ptr::read_volatile(&(*BOOT_INFO).single_kernel) != 0 }
}

pub fn get_cmdsize() -> usize {
	unsafe { core::ptr::read_volatile(&(*BOOT_INFO).cmdsize) as usize }
}

pub fn get_cmdline() -> VirtAddr {
	VirtAddr(unsafe { core::ptr::read_volatile(&(*BOOT_INFO).cmdline) })
}

/// Earliest initialization function called by the Boot Processor.
pub fn message_output_init() {
	percore::init();

	unsafe {
		COM1.port_address = core::ptr::read_volatile(&(*BOOT_INFO).uartport);
	}

	if env::is_single_kernel() {
		// We can only initialize the serial port here, because VGA requires processor
		// configuration first.
		unsafe {
			COM1.init(SERIAL_PORT_BAUDRATE);
		}
	}
}

pub fn output_message_byte(byte: u8) {
	if env::is_single_kernel() {
		// Output messages to the serial port.
		unsafe {
			COM1.write_byte(byte);
		}
	} else {
		// Output messages to the kernel message buffer in multi-kernel mode.
		kernel_message_buffer::write_byte(byte);
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

	if env::is_single_kernel() && !env::is_uhyve() {
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
