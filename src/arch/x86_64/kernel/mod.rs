// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//                    Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use alloc::collections::BTreeMap;
use core::convert::TryInto;
#[cfg(feature = "newlib")]
use core::slice;
use core::{intrinsics, ptr};

use x86::controlregs::{cr0, cr0_write, cr4, Cr0};

use crate::arch::mm::VirtAddr;
use crate::arch::x86_64::kernel::irq::{get_irq_name, IrqStatistics};
use crate::arch::x86_64::kernel::percore::*;
use crate::arch::x86_64::kernel::serial::SerialPort;
use crate::environment;
use crate::kernel_message_buffer;
use crate::scheduler::CoreId;

#[cfg(feature = "acpi")]
pub mod acpi;
pub mod apic;
#[cfg(feature = "pci")]
pub mod fuse;
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
#[cfg(feature = "smp")]
mod smp_boot_code;
pub mod systemtime;
#[cfg(feature = "vga")]
mod vga;

#[cfg(not(test))]
global_asm!(include_str!("start.s"));
#[cfg(all(not(test), not(fsgsbase)))]
global_asm!(include_str!("switch.s"));
#[cfg(all(not(test), fsgsbase))]
global_asm!(include_str!("switch_fsgsbase.s"));

const SERIAL_PORT_BAUDRATE: u32 = 115_200;

/// Map between Core ID and per-core scheduler
static mut IRQ_COUNTERS: BTreeMap<CoreId, &IrqStatistics> = BTreeMap::new();
const BOOTINFO_MAGIC_NUMBER: u32 = 0xC0DE_CAFEu32;

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
#[cfg(not(target_os = "hermit"))]
static mut BOOT_INFO: *mut BootInfo = ptr::null_mut();

#[cfg(all(target_os = "hermit", not(feature = "newlib")))]
#[link_section = ".data"]
static mut BOOT_INFO: *mut BootInfo = ptr::null_mut();

#[cfg(all(target_os = "hermit", feature = "newlib"))]
#[link_section = ".mboot"]
static mut BOOT_INFO: *mut BootInfo = ptr::null_mut();

/// Serial port to print kernel messages
static mut COM1: SerialPort = SerialPort::new(0x3f8);

pub fn has_ipdevice() -> bool {
	let ip = unsafe { core::ptr::read_volatile(&(*BOOT_INFO).hcip) };

	!(ip[0] == 255 && ip[1] == 255 && ip[2] == 255 && ip[3] == 255)
}

#[cfg(not(feature = "newlib"))]
pub fn uhyve_get_ip() -> [u8; 4] {
	unsafe { core::ptr::read_volatile(&(*BOOT_INFO).hcip) }
}

#[no_mangle]
#[cfg(not(feature = "newlib"))]
pub fn sys_uhyve_get_ip() -> [u8; 4] {
	kernel_function!(uhyve_get_ip())
}

#[cfg(not(feature = "newlib"))]
pub fn uhyve_get_gateway() -> [u8; 4] {
	unsafe { core::ptr::read_volatile(&(*BOOT_INFO).hcgateway) }
}

#[no_mangle]
#[cfg(not(feature = "newlib"))]
pub fn sys_uhyve_get_gateway() -> [u8; 4] {
	kernel_function!(uhyve_get_gateway())
}

#[cfg(not(feature = "newlib"))]
pub fn uhyve_get_mask() -> [u8; 4] {
	unsafe { core::ptr::read_volatile(&(*BOOT_INFO).hcmask) }
}

#[no_mangle]
#[cfg(not(feature = "newlib"))]
pub fn sys_uhyve_get_mask() -> [u8; 4] {
	kernel_function!(uhyve_get_mask())
}

#[no_mangle]
#[cfg(feature = "newlib")]
pub unsafe extern "C" fn sys_uhyve_get_ip(ip: *mut u8) {
	switch_to_kernel!();
	let data = core::ptr::read_volatile(&(*BOOT_INFO).hcip);
	slice::from_raw_parts_mut(ip, 4).copy_from_slice(&data);
	switch_to_user!();
}

#[no_mangle]
#[cfg(feature = "newlib")]
pub unsafe extern "C" fn sys_uhyve_get_gateway(gw: *mut u8) {
	switch_to_kernel!();
	let data = core::ptr::read_volatile(&(*BOOT_INFO).hcgateway);
	slice::from_raw_parts_mut(gw, 4).copy_from_slice(&data);
	switch_to_user!();
}

#[no_mangle]
#[cfg(feature = "newlib")]
pub unsafe extern "C" fn sys_uhyve_get_mask(mask: *mut u8) {
	switch_to_kernel!();
	let data = core::ptr::read_volatile(&(*BOOT_INFO).hcmask);
	slice::from_raw_parts_mut(mask, 4).copy_from_slice(&data);
	switch_to_user!();
}

pub fn get_base_address() -> VirtAddr {
	unsafe { VirtAddr(core::ptr::read_volatile(&(*BOOT_INFO).base)) }
}

pub fn get_image_size() -> usize {
	unsafe { core::ptr::read_volatile(&(*BOOT_INFO).image_size) as usize }
}

pub fn get_limit() -> usize {
	unsafe { core::ptr::read_volatile(&(*BOOT_INFO).limit) as usize }
}

pub fn get_tls_start() -> VirtAddr {
	unsafe { VirtAddr(core::ptr::read_volatile(&(*BOOT_INFO).tls_start)) }
}

pub fn get_tls_filesz() -> usize {
	unsafe { core::ptr::read_volatile(&(*BOOT_INFO).tls_filesz) as usize }
}

pub fn get_tls_memsz() -> usize {
	unsafe { core::ptr::read_volatile(&(*BOOT_INFO).tls_memsz) as usize }
}

pub fn get_mbinfo() -> VirtAddr {
	unsafe { VirtAddr(core::ptr::read_volatile(&(*BOOT_INFO).mb_info)) }
}

#[cfg(feature = "smp")]
pub fn get_processor_count() -> u32 {
	unsafe { core::ptr::read_volatile(&(*BOOT_INFO).cpu_online) as u32 }
}

#[cfg(not(feature = "smp"))]
pub fn get_processor_count() -> u32 {
	1
}

/// Whether HermitCore is running under the "uhyve" hypervisor.
pub fn is_uhyve() -> bool {
	unsafe { core::ptr::read_volatile(&(*BOOT_INFO).uhyve) & 0x1 == 0x1 }
}

pub fn is_uhyve_with_pci() -> bool {
	unsafe { core::ptr::read_volatile(&(*BOOT_INFO).uhyve) & 0x3 == 0x3 }
}

/// Whether HermitCore is running alone (true) or side-by-side to Linux in Multi-Kernel mode (false).
pub fn is_single_kernel() -> bool {
	unsafe { core::ptr::read_volatile(&(*BOOT_INFO).single_kernel) != 0 }
}

pub fn get_cmdsize() -> usize {
	unsafe { core::ptr::read_volatile(&(*BOOT_INFO).cmdsize) as usize }
}

pub fn get_cmdline() -> VirtAddr {
	unsafe { VirtAddr(core::ptr::read_volatile(&(*BOOT_INFO).cmdline)) }
}

/// Earliest initialization function called by the Boot Processor.
pub fn message_output_init() {
	percore::init();

	unsafe {
		COM1.port_address = core::ptr::read_volatile(&(*BOOT_INFO).uartport);
	}

	if environment::is_single_kernel() {
		// We can only initialize the serial port here, because VGA requires processor
		// configuration first.
		unsafe {
			COM1.init(SERIAL_PORT_BAUDRATE);
		}
	}
}

#[cfg(all(not(target_os = "hermit"), not(target_os = "windows")))]
pub fn output_message_byte(byte: u8) {
	extern "C" {
		fn write(fd: i32, buf: *const u8, count: usize) -> isize;
	}

	unsafe {
		let _ = write(2, &byte as *const _, 1);
	}
}

#[cfg(target_os = "windows")]
pub fn output_message_byte(byte: u8) {
	extern "C" {
		fn _write(fd: i32, buf: *const u8, count: u32) -> isize;
	}

	unsafe {
		let _ = _write(2, &byte as *const _, 1);
	}
}

#[cfg(not(target_os = "hermit"))]
#[test]
fn test_output() {
	output_message_byte('t' as u8);
	output_message_byte('e' as u8);
	output_message_byte('s' as u8);
	output_message_byte('t' as u8);
	output_message_byte('\n' as u8);
}

#[cfg(target_os = "hermit")]
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

//#[cfg(target_os = "hermit")]
pub fn output_message_buf(buf: &[u8]) {
	for byte in buf {
		output_message_byte(*byte);
	}
}

/// Real Boot Processor initialization as soon as we have put the first Welcome message on the screen.
#[cfg(target_os = "hermit")]
pub fn boot_processor_init() {
	processor::detect_features();
	processor::configure();

	if cfg!(feature = "vga") && environment::is_single_kernel() && !environment::is_uhyve() {
		#[cfg(feature = "vga")]
		vga::init();
	}

	crate::mm::init();
	crate::mm::print_information();
	environment::init();
	gdt::init();
	gdt::add_current_core();
	idt::install();
	pic::init();

	irq::install();
	processor::detect_frequency();
	processor::print_information();
	unsafe {
		trace!("Cr0: 0x{:x}, Cr4: 0x{:x}", cr0(), cr4());
	}
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
#[cfg(target_os = "hermit")]
pub fn boot_application_processors() {
	#[cfg(feature = "smp")]
	apic::boot_application_processors();
	apic::print_information();
}

/// Application Processor initialization
#[cfg(all(target_os = "hermit", feature = "smp"))]
pub fn application_processor_init() {
	percore::init();
	processor::configure();
	gdt::add_current_core();
	idt::install();
	apic::init_x2apic();
	apic::init_local_apic();
	unsafe {
		trace!("Cr0: 0x{:x}, Cr4: 0x{:x}", cr0(), cr4());
	}
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

pub fn print_statistics() {
	info!("Number of interrupts");
	unsafe {
		for (core_id, irg_statistics) in IRQ_COUNTERS.iter() {
			for (i, counter) in irg_statistics.counters.iter().enumerate() {
				if *counter > 0 {
					match get_irq_name(i.try_into().unwrap()) {
						Some(name) => {
							info!("[{}][{}]: {}", core_id, name, *counter);
						}
						_ => {
							info!("[{}][{}]: {}", core_id, i, *counter);
						}
					}
				}
			}
		}
	}
}

#[cfg(target_os = "hermit")]
#[inline(never)]
#[no_mangle]
unsafe fn pre_init(boot_info: &'static mut BootInfo) -> ! {
	assert_eq!(boot_info.magic_number, BOOTINFO_MAGIC_NUMBER);
	// Enable caching
	let mut cr0 = cr0();
	cr0.remove(Cr0::CR0_CACHE_DISABLE | Cr0::CR0_NOT_WRITE_THROUGH);
	cr0_write(cr0);

	BOOT_INFO = boot_info as *mut BootInfo;

	if boot_info.cpu_online == 0 {
		crate::boot_processor_main()
	} else {
		#[cfg(not(feature = "smp"))]
		{
			error!("SMP support deactivated");
			loop {
				processor::halt();
			}
		}
		#[cfg(feature = "smp")]
		crate::application_processor_main();
	}
}
