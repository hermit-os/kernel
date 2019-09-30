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
pub mod pci;
mod pci_ids;
pub mod percore;
pub mod pic;
pub mod pit;
pub mod processor;
pub mod scheduler;
pub mod serial;
#[cfg(not(test))]
mod smp_boot_code;
#[cfg(not(test))]
mod start;
pub mod switch;
pub mod systemtime;
#[cfg(feature = "vga")]
mod vga;

use arch::x86_64::kernel::percore::*;
use arch::x86_64::kernel::serial::SerialPort;

use core::{intrinsics, ptr};
use environment;
use kernel_message_buffer;

const SERIAL_PORT_BAUDRATE: u32 = 115_200;

#[repr(C)]
pub struct BootInfo {
	magic_number: u32,
	version: u32,
	base: u64,
	limit: u64,
	image_size: u64,
	tls_start: u64,
	tls_filesz: u64,
	tls_memsz: u64,
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
}

/// Kernel header to announce machine features
#[cfg(not(feature = "newlib"))]
#[link_section = ".data"]
static mut BOOT_INFO: *mut BootInfo = ptr::null_mut();

#[cfg(feature = "newlib")]
#[link_section = ".mboot"]
static mut BOOT_INFO: *mut BootInfo = ptr::null_mut();

/// Serial port to print kernel messages
static mut COM1: SerialPort = SerialPort::new(0x3f8);

pub fn get_ip() -> [u8; 4] {
	let mut ip: [u8; 4] = [0, 0, 0, 0];

	for i in 0..4 {
		ip[i] = unsafe { intrinsics::volatile_load(&(*BOOT_INFO).hcip[i]) as u8 };
	}

	ip
}

pub fn get_gateway() -> [u8; 4] {
	let mut gw: [u8; 4] = [0, 0, 0, 0];

	for i in 0..4 {
		gw[i] = unsafe { intrinsics::volatile_load(&(*BOOT_INFO).hcgateway[i]) as u8 };
	}

	gw
}

pub fn get_base_address() -> usize {
	unsafe { intrinsics::volatile_load(&(*BOOT_INFO).base) as usize }
}

pub fn get_image_size() -> usize {
	unsafe { intrinsics::volatile_load(&(*BOOT_INFO).image_size) as usize }
}

pub fn get_limit() -> usize {
	unsafe { intrinsics::volatile_load(&(*BOOT_INFO).limit) as usize }
}

pub fn get_tls_start() -> usize {
	unsafe { intrinsics::volatile_load(&(*BOOT_INFO).tls_start) as usize }
}

pub fn get_tls_filesz() -> usize {
	unsafe { intrinsics::volatile_load(&(*BOOT_INFO).tls_filesz) as usize }
}

pub fn get_tls_memsz() -> usize {
	unsafe { intrinsics::volatile_load(&(*BOOT_INFO).tls_memsz) as usize }
}

pub fn get_mbinfo() -> usize {
	unsafe { intrinsics::volatile_load(&(*BOOT_INFO).mb_info) as usize }
}

pub fn get_processor_count() -> usize {
	unsafe { intrinsics::volatile_load(&(*BOOT_INFO).cpu_online) as usize }
}

/// Whether HermitCore is running under the "uhyve" hypervisor.
pub fn is_uhyve() -> bool {
	unsafe { intrinsics::volatile_load(&(*BOOT_INFO).uhyve) != 0 }
}

/// Whether HermitCore is running alone (true) or side-by-side to Linux in Multi-Kernel mode (false).
pub fn is_single_kernel() -> bool {
	unsafe { intrinsics::volatile_load(&(*BOOT_INFO).single_kernel) != 0 }
}

pub fn get_cmdsize() -> usize {
	unsafe { intrinsics::volatile_load(&(*BOOT_INFO).cmdsize) as usize }
}

pub fn get_cmdline() -> usize {
	unsafe { intrinsics::volatile_load(&(*BOOT_INFO).cmdline) as usize }
}

/// Earliest initialization function called by the Boot Processor.
pub fn message_output_init() {
	percore::init();

	unsafe {
		COM1.port_address = intrinsics::volatile_load(&(*BOOT_INFO).uartport);
	}

	if environment::is_single_kernel() {
		// We can only initialize the serial port here, because VGA requires processor
		// configuration first.
		unsafe {
			COM1.init(SERIAL_PORT_BAUDRATE);
		}
	}
}

#[cfg(test)]
pub fn output_message_byte(byte: u8) {
	extern "C" {
		fn write(fd: i32, buf: *const u8, count: usize) -> isize;
	}

	unsafe {
		let _ = write(2, &byte as *const _, 1);
	}
}

#[test]
fn test_output() {
	output_message_byte('t' as u8);
	output_message_byte('e' as u8);
	output_message_byte('s' as u8);
	output_message_byte('t' as u8);
	output_message_byte('\n' as u8);
}

#[cfg(not(test))]
pub fn output_message_byte(byte: u8) {
	if environment::is_single_kernel() {
		// Output messages to the serial port and VGA screen in unikernel mode.
		unsafe {
			COM1.write_byte(byte);
		}

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
#[cfg(not(test))]
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
#[cfg(not(test))]
pub fn boot_application_processors() {
	apic::boot_application_processors();
	apic::print_information();
}

/// Application Processor initialization
#[cfg(not(test))]
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
		let _ = intrinsics::atomic_xadd(&mut (*BOOT_INFO).cpu_online as *mut u32, 1);
	}
}
