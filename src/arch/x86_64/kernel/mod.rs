use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use x86_64::registers::control::{Cr0, Cr4};

pub(crate) use self::apic::{set_oneshot_timer, wakeup_core};
use crate::arch::kernel::core_local::*;
#[cfg(feature = "uhyve")]
use crate::env::{self, UhyveStartInfo};

#[cfg(feature = "acpi")]
mod acpi;
pub mod apic;
pub mod core_local;
pub mod gdt;
pub mod interrupts;
#[cfg(feature = "kernel-stack")]
pub mod kernel_stack;
#[cfg(all(not(feature = "pci"), feature = "virtio"))]
pub mod mmio;
#[cfg(feature = "pc-keyboard")]
pub mod pc_keyboard;
#[cfg(feature = "pci")]
pub mod pci;
pub mod pic;
pub mod pit;
pub mod processor;
pub mod scheduler;
pub mod serial;
pub mod switch;
pub(crate) mod systemtime;
#[cfg(feature = "vga")]
pub mod vga;

#[cfg(feature = "smp")]
pub fn get_possible_cpus() -> u32 {
	#[cfg(feature = "uhyve")]
	if let Some(num_cpus) = env::start_info().uhyve_num_cpus() {
		return num_cpus.get().try_into().unwrap();
	}

	apic::local_apic_id_count()
}

#[cfg(feature = "smp")]
pub fn get_processor_count() -> u32 {
	CPU_ONLINE.load(Ordering::Acquire)
}

#[cfg(not(feature = "smp"))]
pub fn get_processor_count() -> u32 {
	1
}

/// Real Boot Processor initialization as soon as we have put the first Welcome message on the screen.
#[cfg(target_os = "none")]
pub fn boot_processor_init() {
	processor::detect_features();
	processor::configure();

	#[cfg(feature = "vga")]
	vga::init();

	crate::mm::init();
	crate::mm::print_information();
	CoreLocal::get().add_irq_counter();
	gdt::add_current_core();
	interrupts::load_idt();
	pic::init();

	processor::detect_frequency();
	crate::logging::KERNEL_LOGGER.set_time(true);
	processor::print_information();
	debug!("Cr0 = {:?}", Cr0::read());
	debug!("Cr4 = {:?}", Cr4::read());
	interrupts::install();
	systemtime::init();

	#[cfg(feature = "acpi")]
	acpi::init();

	#[cfg(feature = "pci")]
	pci::init();

	apic::init();
	scheduler::install_timer_handler();
	finish_processor_init();
}

/// Application Processor initialization
#[cfg(all(target_os = "none", feature = "smp"))]
pub fn application_processor_init() {
	CoreLocal::install();
	processor::configure();
	gdt::add_current_core();
	interrupts::load_idt();
	if processor::supports_x2apic() {
		apic::init_x2apic();
	}
	apic::init_local_apic();
	debug!("Cr0 = {:?}", Cr0::read());
	debug!("Cr4 = {:?}", Cr4::read());
	finish_processor_init();
}

fn finish_processor_init() {
	#[cfg(feature = "uhyve")]
	if env::start_info().is_uhyve() {
		// uhyve does not use apic::detect_from_acpi and therefore does not know the number of processors and
		// their APIC IDs in advance.
		// Therefore, we have to add each booted processor into the CPU_LOCAL_APIC_IDS vector ourselves.
		// Fortunately, the Local APIC IDs of uhyve are sequential and therefore match the Core IDs.
		apic::add_local_apic_id(core_id() as u8);

		// uhyve also boots each processor into _start itself and does not use apic::boot_application_processors.
		// Therefore, the current processor already needs to prepare the processor variables for a possible next processor.
		#[cfg(feature = "smp")]
		apic::init_next_processor_variables();
	}
}

pub fn boot_next_processor() {
	// This triggers apic::boot_application_processors (bare-metal/QEMU) or uhyve
	// to initialize the next processor.
	let cpu_online = CPU_ONLINE.fetch_add(1, Ordering::Release);

	#[cfg(feature = "uhyve")]
	if env::start_info().is_uhyve() {
		return;
	}

	if cpu_online == 0 {
		#[cfg(all(target_os = "none", feature = "smp"))]
		apic::boot_application_processors();
	}

	if !cfg!(feature = "smp") {
		apic::print_information();
	}
}

pub fn print_statistics() {
	interrupts::print_statistics();
}

/// `CPU_ONLINE` is the count of CPUs that finished initialization.
///
/// It also synchronizes initialization of CPU cores.
pub static CPU_ONLINE: AtomicU32 = AtomicU32::new(0);

pub static CURRENT_STACK_ADDRESS: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());
