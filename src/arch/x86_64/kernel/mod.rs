// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//                    Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

#[cfg(feature = "acpi")]
pub mod acpi;
pub mod apic;
pub mod gdt;
pub mod idt;
pub mod irq;
#[cfg(feature = "pci")]
pub mod pci;
#[cfg(feature = "pci")]
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
pub mod virtio;

use arch::x86_64::kernel::percore::*;
use arch::x86_64::kernel::serial::SerialPort;

#[cfg(feature = "newlib")]
use core::slice;
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
#[cfg(test)]
static mut BOOT_INFO: *mut BootInfo = ptr::null_mut();

#[cfg(all(not(test), not(feature = "newlib")))]
#[link_section = ".data"]
static mut BOOT_INFO: *mut BootInfo = ptr::null_mut();

#[cfg(all(not(test), feature = "newlib"))]
#[link_section = ".mboot"]
static mut BOOT_INFO: *mut BootInfo = ptr::null_mut();

/// Serial port to print kernel messages
static mut COM1: SerialPort = SerialPort::new(0x3f8);

pub fn has_ipdevice() -> bool {
	let ip = unsafe { intrinsics::volatile_load(&(*BOOT_INFO).hcip) };

	if ip[0] == 255 && ip[1] == 255 && ip[2] == 255 && ip[3] == 255 {
		false
	} else {
		true
	}
}

#[no_mangle]
#[cfg(not(feature = "newlib"))]
pub fn uhyve_get_ip() -> [u8; 4] {
	unsafe { intrinsics::volatile_load(&(*BOOT_INFO).hcip) }
}

#[no_mangle]
#[cfg(not(feature = "newlib"))]
pub fn uhyve_get_gateway() -> [u8; 4] {
	unsafe { intrinsics::volatile_load(&(*BOOT_INFO).hcgateway) }
}

#[no_mangle]
#[cfg(not(feature = "newlib"))]
pub fn uhyve_get_mask() -> [u8; 4] {
	unsafe { intrinsics::volatile_load(&(*BOOT_INFO).hcmask) }
}

#[no_mangle]
#[cfg(feature = "newlib")]
pub unsafe extern "C" fn uhyve_get_ip(ip: *mut u8) {
	let data = intrinsics::volatile_load(&(*BOOT_INFO).hcip);
	slice::from_raw_parts_mut(ip, 4).copy_from_slice(&data);
}

#[no_mangle]
#[cfg(feature = "newlib")]
pub unsafe extern "C" fn uhyve_get_gateway(gw: *mut u8) {
	let data = intrinsics::volatile_load(&(*BOOT_INFO).hcgateway);
	slice::from_raw_parts_mut(gw, 4).copy_from_slice(&data);
}

#[no_mangle]
#[cfg(feature = "newlib")]
pub unsafe extern "C" fn uhyve_get_mask(mask: *mut u8) {
	let data = intrinsics::volatile_load(&(*BOOT_INFO).hcmask);
	slice::from_raw_parts_mut(mask, 4).copy_from_slice(&data);
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

pub fn get_processor_count() -> u32 {
	unsafe { intrinsics::volatile_load(&(*BOOT_INFO).cpu_online) as u32 }
}

/// Whether HermitCore is running under the "uhyve" hypervisor.
pub fn is_uhyve() -> bool {
	unsafe { intrinsics::volatile_load(&(*BOOT_INFO).uhyve) & 0x1 == 0x1 }
}

pub fn is_uhyve_with_pci() -> bool {
	unsafe { intrinsics::volatile_load(&(*BOOT_INFO).uhyve) & 0x3 == 0x3 }
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

#[cfg(all(test, not(target_os = "windows")))]
pub fn output_message_byte(byte: u8) {
	extern "C" {
		fn write(fd: i32, buf: *const u8, count: usize) -> isize;
	}

	unsafe {
		let _ = write(2, &byte as *const _, 1);
	}
}

#[cfg(all(test, target_os = "windows"))]
pub fn output_message_byte(byte: u8) {
	extern "C" {
		fn _write(fd: i32, buf: *const u8, count: u32) -> isize;
	}

	unsafe {
		let _ = _write(2, &byte as *const _, 1);
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
	pic::init();

	irq::install();
	processor::detect_frequency();
	processor::print_information();
	systemtime::init();

	if environment::is_single_kernel() {
		if is_uhyve_with_pci() || !is_uhyve() {
			#[cfg(feature = "pci")]
			pci::init();
			#[cfg(feature = "pci")]
			pci::print_information();
		}
		if !environment::is_uhyve() {
			#[cfg(feature = "acpi")]
			acpi::init();
		}
	}

	apic::init();
	scheduler::install_timer_handler();
	finish_processor_init();
	irq::enable();
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
