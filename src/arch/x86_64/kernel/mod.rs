#[cfg(feature = "newlib")]
use core::slice;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use hermit_entry::boot_info::{BootInfo, PlatformInfo, RawBootInfo};
use hermit_sync::InterruptSpinMutex;
use x86::controlregs::{cr0, cr0_write, cr4, Cr0};

use self::serial::SerialPort;
use crate::arch::mm::{PhysAddr, VirtAddr};
use crate::arch::x86_64::kernel::core_local::*;
use crate::env;

#[cfg(feature = "acpi")]
pub mod acpi;
pub mod apic;
pub mod core_local;
#[cfg(feature = "pci")]
pub mod fuse;
pub mod gdt;
pub mod interrupts;
#[cfg(not(feature = "pci"))]
pub mod mmio;
#[cfg(feature = "pci")]
pub mod pci;
pub mod pic;
pub mod pit;
pub mod processor;
pub mod scheduler;
pub mod serial;
#[cfg(target_os = "none")]
mod start;
pub mod switch;
pub mod systemtime;
#[cfg(feature = "vga")]
mod vga;

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

#[cfg(feature = "smp")]
pub fn raw_boot_info() -> &'static RawBootInfo {
	unsafe { RAW_BOOT_INFO.unwrap() }
}

/// Serial port to print kernel messages
static COM1: InterruptSpinMutex<Option<SerialPort>> = InterruptSpinMutex::new(None);

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

pub fn get_mbinfo() -> VirtAddr {
	match boot_info().platform_info {
		PlatformInfo::Multiboot {
			multiboot_info_addr,
			..
		} => VirtAddr(multiboot_info_addr.get()),
		PlatformInfo::Uhyve { .. } => VirtAddr(0),
	}
}

#[cfg(feature = "smp")]
pub fn get_possible_cpus() -> u32 {
	use core::cmp;

	match boot_info().platform_info {
		PlatformInfo::Multiboot { .. } => apic::local_apic_id_count(),
		// FIXME: Remove get_processor_count after a transition period for uhyve 0.1.3 adoption
		PlatformInfo::Uhyve { num_cpus, .. } => cmp::max(
			u32::try_from(num_cpus.get()).unwrap(),
			get_processor_count(),
		),
	}
}

#[cfg(feature = "smp")]
pub fn get_processor_count() -> u32 {
	CPU_ONLINE.load(Ordering::Acquire)
}

#[cfg(not(feature = "smp"))]
pub fn get_processor_count() -> u32 {
	1
}

/// Whether HermitCore is running under the "uhyve" hypervisor.
pub fn is_uhyve() -> bool {
	matches!(boot_info().platform_info, PlatformInfo::Uhyve { .. })
}

pub fn is_uhyve_with_pci() -> bool {
	match boot_info().platform_info {
		PlatformInfo::Multiboot { .. } => false,
		PlatformInfo::Uhyve { has_pci, .. } => has_pci,
	}
}

pub fn get_cmdsize() -> usize {
	match boot_info().platform_info {
		PlatformInfo::Multiboot { command_line, .. } => command_line
			.map(|command_line| command_line.len())
			.unwrap_or_default(),
		PlatformInfo::Uhyve { .. } => 0,
	}
}

pub fn get_cmdline() -> VirtAddr {
	match boot_info().platform_info {
		PlatformInfo::Multiboot { command_line, .. } => VirtAddr(
			command_line
				.map(|command_line| command_line.as_ptr() as u64)
				.unwrap_or_default(),
		),
		PlatformInfo::Uhyve { .. } => VirtAddr(0),
	}
}

// We can only initialize the serial port here, because VGA requires processor
// configuration first.
/// Earliest initialization function called by the Boot Processor.
pub fn message_output_init() {
	CoreLocal::install();

	let base = boot_info().hardware_info.serial_port_base.unwrap().get();
	let serial_port = unsafe { SerialPort::new(base) };
	*COM1.lock() = Some(serial_port);
}

#[cfg(target_os = "none")]
pub fn output_message_buf(buf: &[u8]) {
	// Output messages to the serial port and VGA screen in unikernel mode.
	COM1.lock().as_mut().unwrap().send(buf);

	#[cfg(feature = "vga")]
	vga::print(buf);
}

#[cfg(not(target_os = "none"))]
pub fn output_message_buf(buf: &[u8]) {
	use std::io::Write;

	std::io::stderr().write_all(buf).unwrap();
}

/// Real Boot Processor initialization as soon as we have put the first Welcome message on the screen.
#[cfg(target_os = "none")]
pub fn boot_processor_init() {
	processor::detect_features();
	processor::configure();

	if cfg!(feature = "vga") && !env::is_uhyve() {
		#[cfg(feature = "vga")]
		vga::init();
	}

	crate::mm::init();
	crate::mm::print_information();
	env::init();
	gdt::add_current_core();
	interrupts::load_idt();
	pic::init();

	processor::detect_frequency();
	processor::print_information();
	unsafe {
		trace!("Cr0: {:#x}, Cr4: {:#x}", cr0(), cr4());
	}
	interrupts::install();
	systemtime::init();

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

	apic::init();
	scheduler::install_timer_handler();
	finish_processor_init();
	interrupts::enable();
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
	CoreLocal::install();
	processor::configure();
	gdt::add_current_core();
	interrupts::load_idt();
	apic::init_x2apic();
	apic::init_local_apic();
	unsafe {
		trace!("Cr0: {:#x}, Cr4: {:#x}", cr0(), cr4());
	}
	interrupts::enable();
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
		apic::init_next_processor_variables();
	}

	// This triggers apic::boot_application_processors (bare-metal/QEMU) or uhyve
	// to initialize the next processor.
	CPU_ONLINE.fetch_add(1, Ordering::Release);
}

pub fn print_statistics() {
	interrupts::print_statistics();
}

/// `CPU_ONLINE` is the count of CPUs that finished initialization.
///
/// It also synchronizes initialization of CPU cores.
pub static CPU_ONLINE: AtomicU32 = AtomicU32::new(0);

pub static CURRENT_STACK_ADDRESS: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "none")]
#[inline(never)]
#[no_mangle]
unsafe extern "C" fn pre_init(boot_info: &'static RawBootInfo, cpu_id: u32) -> ! {
	// Enable caching
	unsafe {
		let mut cr0 = cr0();
		cr0.remove(Cr0::CR0_CACHE_DISABLE | Cr0::CR0_NOT_WRITE_THROUGH);
		cr0_write(cr0);
	}

	unsafe {
		RAW_BOOT_INFO = Some(boot_info);
		BOOT_INFO = Some(BootInfo::from(*boot_info));
	}

	if cpu_id == 0 {
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

#[cfg(all(test, not(target_os = "none")))]
mod tests {
	use super::*;

	#[test]
	fn test_output() {
		output_message_buf(b"test message\n");
	}
}
