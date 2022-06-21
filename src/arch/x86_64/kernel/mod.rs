use alloc::collections::BTreeMap;
#[cfg(feature = "newlib")]
use core::slice;

use hermit_entry::{BootInfo, RawBootInfo};
use x86::controlregs::{cr0, cr0_write, cr4, Cr0};

use crate::arch::mm::{PhysAddr, VirtAddr};
use crate::arch::x86_64::kernel::irq::{get_irq_name, IrqStatistics};
use crate::arch::x86_64::kernel::percore::*;
use crate::arch::x86_64::kernel::serial::SerialPort;
use crate::env;
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
#[cfg(not(feature = "pci"))]
pub mod mmio;
#[cfg(feature = "pci")]
pub mod pci;
pub mod percore;
pub mod pic;
pub mod pit;
pub mod processor;
pub mod scheduler;
pub mod serial;
#[cfg(not(test))]
mod start;
pub mod switch;
pub mod systemtime;
#[cfg(feature = "vga")]
mod vga;

const SERIAL_PORT_BAUDRATE: u32 = 115_200;

/// Map between Core ID and per-core scheduler
static mut IRQ_COUNTERS: BTreeMap<CoreId, &IrqStatistics> = BTreeMap::new();

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

/// Serial port to print kernel messages
static mut COM1: SerialPort = SerialPort::new(0x3f8);

#[cfg(feature = "newlib")]
extern "C" fn __sys_uhyve_get_ip(ip: *mut u8) {
	let data = boot_info().net_info.ip;
	unsafe {
		slice::from_raw_parts_mut(ip, 4).copy_from_slice(&data);
	}
}

#[no_mangle]
#[cfg(feature = "newlib")]
pub unsafe extern "C" fn sys_uhyve_get_ip(ip: *mut u8) {
	kernel_function!(__sys_uhyve_get_ip(ip))
}

#[cfg(feature = "newlib")]
extern "C" fn __sys_uhyve_get_gateway(gw: *mut u8) {
	let data = boot_info().net_info.gateway;
	unsafe {
		slice::from_raw_parts_mut(gw, 4).copy_from_slice(&data);
	}
}

#[no_mangle]
#[cfg(feature = "newlib")]
pub unsafe extern "C" fn sys_uhyve_get_gateway(gw: *mut u8) {
	kernel_function!(__sys_uhyve_get_gateway(gw))
}

#[cfg(feature = "newlib")]
extern "C" fn __sys_uhyve_get_mask(mask: *mut u8) {
	let data = boot_info().net_info.mask;
	unsafe {
		slice::from_raw_parts_mut(mask, 4).copy_from_slice(&data);
	}
}

#[no_mangle]
#[cfg(feature = "newlib")]
pub unsafe extern "C" fn sys_uhyve_get_mask(mask: *mut u8) {
	kernel_function!(__sys_uhyve_get_mask(mask))
}

pub fn get_ram_address() -> PhysAddr {
	PhysAddr(0)
}

pub fn get_base_address() -> VirtAddr {
	VirtAddr(boot_info().base)
}

pub fn get_image_size() -> usize {
	boot_info().image_size as usize
}

pub fn get_limit() -> usize {
	boot_info().limit as usize
}

pub fn get_tls_start() -> VirtAddr {
	VirtAddr(boot_info().tls_info.start)
}

pub fn get_tls_filesz() -> usize {
	boot_info().tls_info.filesz as usize
}

pub fn get_tls_memsz() -> usize {
	boot_info().tls_info.memsz as usize
}

pub fn get_tls_align() -> usize {
	boot_info().tls_info.align as usize
}

pub fn get_mbinfo() -> VirtAddr {
	VirtAddr(boot_info().mb_info)
}

#[cfg(feature = "smp")]
pub fn get_processor_count() -> u32 {
	raw_boot_info().load_cpu_online()
}

#[cfg(not(feature = "smp"))]
pub fn get_processor_count() -> u32 {
	1
}

/// Whether HermitCore is running under the "uhyve" hypervisor.
pub fn is_uhyve() -> bool {
	boot_info().uhyve & 0b1 == 0b1
}

pub fn is_uhyve_with_pci() -> bool {
	boot_info().uhyve & 0b11 == 0b11
}

/// Whether HermitCore is running alone (true) or side-by-side to Linux in Multi-Kernel mode (false).
pub fn is_single_kernel() -> bool {
	boot_info().single_kernel != 0
}

pub fn get_cmdsize() -> usize {
	boot_info().cmdsize as usize
}

pub fn get_cmdline() -> VirtAddr {
	VirtAddr(boot_info().cmdline)
}

/// Earliest initialization function called by the Boot Processor.
pub fn message_output_init() {
	percore::init();

	unsafe {
		COM1.port_address = boot_info().uartport;
	}

	if env::is_single_kernel() {
		// We can only initialize the serial port here, because VGA requires processor
		// configuration first.
		unsafe {
			COM1.init(SERIAL_PORT_BAUDRATE);
		}
	}
}

#[cfg(all(not(target_os = "none"), not(target_os = "windows")))]
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

#[cfg(not(target_os = "none"))]
#[test]
fn test_output() {
	output_message_byte('t' as u8);
	output_message_byte('e' as u8);
	output_message_byte('s' as u8);
	output_message_byte('t' as u8);
	output_message_byte('\n' as u8);
}

#[cfg(target_os = "none")]
pub fn output_message_byte(byte: u8) {
	if env::is_single_kernel() {
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

//#[cfg(target_os = "none")]
pub fn output_message_buf(buf: &[u8]) {
	for byte in buf {
		output_message_byte(*byte);
	}
}

/// Real Boot Processor initialization as soon as we have put the first Welcome message on the screen.
#[cfg(target_os = "none")]
pub fn boot_processor_init() {
	processor::detect_features();
	processor::configure();

	if cfg!(feature = "vga") && env::is_single_kernel() && !env::is_uhyve() {
		#[cfg(feature = "vga")]
		vga::init();
	}

	crate::mm::init();
	crate::mm::print_information();
	env::init();
	gdt::init();
	gdt::add_current_core();
	idt::install();
	pic::init();

	processor::detect_frequency();
	processor::print_information();
	unsafe {
		trace!("Cr0: {:#x}, Cr4: {:#x}", cr0(), cr4());
	}
	irq::install();
	systemtime::init();

	if env::is_single_kernel() {
		if is_uhyve_with_pci() || !is_uhyve() {
			#[cfg(feature = "pci")]
			pci::init();
			#[cfg(feature = "pci")]
			pci::print_information();
		}
		if !env::is_uhyve() {
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
#[cfg(target_os = "none")]
pub fn boot_application_processors() {
	#[cfg(feature = "smp")]
	apic::boot_application_processors();
	apic::print_information();
}

/// Application Processor initialization
#[cfg(all(target_os = "none", feature = "smp"))]
pub fn application_processor_init() {
	percore::init();
	processor::configure();
	gdt::add_current_core();
	idt::install();
	apic::init_x2apic();
	apic::init_local_apic();
	unsafe {
		trace!("Cr0: {:#x}, Cr4: {:#x}", cr0(), cr4());
	}
	irq::enable();
	finish_processor_init();
}

fn finish_processor_init() {
	if env::is_uhyve() {
		// uhyve does not use apic::detect_from_acpi and therefore does not know the number of processors and
		// their APIC IDs in advance.
		// Therefore, we have to add each booted processor into the CPU_LOCAL_APIC_IDS vector ourselves.
		// Fortunately, the Local APIC IDs of uhyve are sequential and therefore match the Core IDs.
		apic::add_local_apic_id(core_id() as u8);

		// uhyve also boots each processor into _start itself and does not use apic::boot_application_processors.
		// Therefore, the current processor already needs to prepare the processor variables for a possible next processor.
		apic::init_next_processor_variables(core_id() + 1);
	}

	// This triggers apic::boot_application_processors (bare-metal/QEMU) or uhyve
	// to initialize the next processor.
	raw_boot_info().increment_cpu_online();
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

#[cfg(target_os = "none")]
#[inline(never)]
#[no_mangle]
unsafe fn pre_init(boot_info: *const RawBootInfo) -> ! {
	// Enable caching
	unsafe {
		let mut cr0 = cr0();
		cr0.remove(Cr0::CR0_CACHE_DISABLE | Cr0::CR0_NOT_WRITE_THROUGH);
		cr0_write(cr0);
	}

	let boot_info = unsafe { RawBootInfo::try_from_ptr(boot_info).unwrap() };
	unsafe {
		RAW_BOOT_INFO = Some(boot_info);
		BOOT_INFO = Some(BootInfo::copy_from(boot_info));
	}

	if boot_info.load_cpu_online() == 0 {
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
