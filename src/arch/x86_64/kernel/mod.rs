// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//                    Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

pub mod acpi;
pub mod apic;
pub mod gdt;
pub mod idt;
pub mod irq;
pub mod percore;
pub mod pci;
pub mod pic;
pub mod pit;
pub mod processor;
pub mod scheduler;
pub mod serial;
pub mod systemtime;
pub mod switch;
mod start;
mod pci_ids;
mod smp_boot_code;
#[cfg(feature = "vga")]
mod vga;

use arch::x86_64::kernel::percore::*;
use arch::x86_64::kernel::serial::SerialPort;

use core::{intrinsics,mem,ptr};
use config::*;
use environment;
use kernel_message_buffer;

const SERIAL_PORT_BAUDRATE: u32 = 115200;

#[repr(C)]
struct KernelHeader {
	magic_number: u32,
	version: u32,
	base: u64,
	limit: u64,
	image_size: u64,
	current_stack_address: u64,
	current_percore_address: u64,
	host_logical_addr: u64,
	boot_gtod: u64,
	mb_info: u64,
	cmdline: u64,
	cmdsize: u64,
	cpu_freq: u32,
	boot_processor: u32,
	cpu_online: u32,
	possible_cpus: u32,
	current_boot_id: u32,
	uartport: u16,
	single_kernel: u8,
	uhyve: u8,
	hcip: [u8; 4],
	hcgateway: [u8; 4],
	hcmask: [u8; 4],
	boot_stack: [u8; KERNEL_STACK_SIZE]
}

/// Kernel header to announce machine features
#[link_section = ".mboot"]
static mut KERNEL_HEADER: KernelHeader = KernelHeader {
	magic_number: 0xC0DECAFEu32,
	version: 0,
	base: 0,
	limit: 0,
	image_size: 0,
	current_stack_address: 0,
	current_percore_address: 0,
	host_logical_addr: 0,
	boot_gtod: 0,
	mb_info: 0,
	cmdline: 0,
	cmdsize: 0,
	cpu_freq: 0,
	boot_processor: !0,
	cpu_online: 0,
	possible_cpus: 0,
	current_boot_id: 0,
	uartport: 0,
	single_kernel: 1,
	uhyve: 0,
	hcip: [10,0,5,2],
	hcgateway: [10,0,5,1],
	hcmask: [255,255,255,0],
	boot_stack: [0x00; KERNEL_STACK_SIZE]
};

static COM1: SerialPort = SerialPort::new(0x3f8);

// FUNCTIONS

pub fn get_image_size() -> usize {
	unsafe { ptr::read_volatile(&KERNEL_HEADER.image_size) as usize }
}

pub fn get_limit() -> usize {
	unsafe { ptr::read_volatile(&KERNEL_HEADER.limit) as usize }
}

pub fn get_mbinfo() -> usize {
	unsafe { ptr::read_volatile(&KERNEL_HEADER.mb_info) as usize }
}

pub fn get_processor_count() -> usize {
	unsafe { ptr::read_volatile(&KERNEL_HEADER.cpu_online) as usize }
}

/// Whether HermitCore is running under the "uhyve" hypervisor.
pub fn is_uhyve() -> bool {
	unsafe { ptr::read_volatile(&KERNEL_HEADER.uhyve) != 0 }
}

/// Whether HermitCore is running alone (true) or side-by-side to Linux in Multi-Kernel mode (false).
pub fn is_single_kernel() -> bool {
	unsafe { ptr::read_volatile(&KERNEL_HEADER.single_kernel) != 0 }
}

pub fn get_cmdsize() -> usize {
	unsafe { ptr::read_volatile(&KERNEL_HEADER.cmdsize) as usize }
}

pub fn get_cmdline() -> usize {
	unsafe { ptr::read_volatile(&KERNEL_HEADER.cmdline) as usize }
}

/// Earliest initialization function called by the Boot Processor.
pub fn message_output_init() {
	percore::init();

	unsafe {
		let port: *mut u16 = mem::transmute(&COM1.port_address);
		*port = ptr::read_volatile(&KERNEL_HEADER.uartport);
	}

	if environment::is_single_kernel() == true {
		// We can only initialize the serial port here, because VGA requires processor
		// configuration first.
		COM1.init(SERIAL_PORT_BAUDRATE);
	}
}

pub fn output_message_byte(byte: u8) {
	if environment::is_single_kernel() {
		// Output messages to the serial port and VGA screen in unikernel mode.
		COM1.write_byte(byte);

		// vga::write_byte() checks if VGA support has been initialized,
		// so we don't need any additional if clause around it.
		#[cfg(feature = "vga")]
		vga::write_byte(byte);
	} else {
		// Output messages to the kernel message buffer in multi-kernel mode.
		kernel_message_buffer::write_byte(byte);
	}
}

/// Real Boot Processor initialization as soon as we have put the first Welcome message on the screen.
pub fn boot_processor_init() {
	processor::detect_features();
	processor::configure();

	if cfg!(feature = "vga") && environment::is_single_kernel() && !environment::is_uhyve() {
		#[cfg(feature = "vga")]
		vga::init();
	}

	::mm::init();
	::mm::print_information();
	environment::init();
	gdt::init();
	gdt::add_current_core();
	idt::install();

	if !environment::is_uhyve() {
		pic::init();
	}

	irq::install();
	processor::enable_fpu_exceptions();
	irq::enable();
	processor::detect_frequency();
	processor::print_information();
	systemtime::init();

	if environment::is_single_kernel() && !environment::is_uhyve() {
		pci::init();
		pci::print_information();
		acpi::init();
	}

	apic::init();
	scheduler::install_timer_handler();
	finish_processor_init();
}

/// Boots all available Application Processors on bare-metal or QEMU.
/// Called after the Boot Processor has been fully initialized along with its scheduler.
pub fn boot_application_processors() {
	apic::boot_application_processors();
	apic::print_information();
}

/// Application Processor initialization
pub fn application_processor_init() {
	percore::init();
	processor::configure();
	gdt::add_current_core();
	idt::install();
	apic::init_x2apic();
	apic::init_local_apic();
	irq::enable();
	finish_processor_init();
}

fn finish_processor_init() {
	debug!("Initialized Processor");

	if environment::is_uhyve() {
		// uhyve does not use apic::detect_from_acpi and therefore does not know the number of processors and
		// their APIC IDs in advance.
		// Therefore, we have to add each booted processor into the CPU_LOCAL_APIC_IDS vector ourselves.
		// Fortunately, the Local APIC IDs of uhyve are sequential and therefore match the Core IDs.
		apic::add_local_apic_id(core_id() as u8);

		// uhyve also boots each processor into entry.asm itself and does not use apic::boot_application_processors.
		// Therefore, the current processor already needs to prepare the processor variables for a possible next processor.
		apic::init_next_processor_variables(core_id() + 1);
	}

	// This triggers apic::boot_application_processors (bare-metal/QEMU) or uhyve
	// to initialize the next processor.
	unsafe {
		let _ = intrinsics::atomic_xadd(&mut KERNEL_HEADER.cpu_online as *mut u32, 1);
	}
}
