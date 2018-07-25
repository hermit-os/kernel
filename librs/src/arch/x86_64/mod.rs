// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//                    Colin Finck, RWTH Aachen University
//
// MIT License
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
// WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

pub mod acpi;
pub mod apic;
pub mod gdt;
pub mod idt;
pub mod irq;
pub mod mm;
pub mod percore;
pub mod pci;
pub mod pic;
pub mod pit;
pub mod processor;
pub mod scheduler;
pub mod serial;
pub mod systemtime;
#[cfg(feature = "vga")]
pub mod vga;

pub use arch::x86_64::apic::set_oneshot_timer;
pub use arch::x86_64::apic::wakeup_core;
pub use arch::x86_64::gdt::set_current_kernel_stack;
pub use arch::x86_64::systemtime::get_boot_time;
use arch::x86_64::percore::*;
use arch::x86_64::serial::SerialPort;
use core::ptr;
use environment;
use kernel_message_buffer;
use synch::spinlock::Spinlock;

const SERIAL_PORT_BAUDRATE: u32 = 115200;


extern "C" {
	static mut cpu_online: u32;
	static uartport: u64;

	fn init_rtl8139_netif(freq: u32) -> i32;
}

lazy_static! {
	static ref COM1: SerialPort =
		SerialPort::new(unsafe { uartport } as u16);
	static ref CPU_ONLINE: Spinlock<&'static mut u32> =
		Spinlock::new(unsafe { &mut cpu_online });
}


// FUNCTIONS

pub fn get_processor_count() -> usize {
	unsafe { ptr::read_volatile(&cpu_online) as usize }
}

/// Earliest initialization function called by the Boot Processor.
pub fn message_output_init() {
	percore::init();

	if environment::is_single_kernel() {
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
	**CPU_ONLINE.lock() += 1;
}

pub fn network_adapter_init() -> i32 {
	// Try initializing the RTL8139 interface using DHCP.
	unsafe { init_rtl8139_netif(processor::get_frequency() as u32) }
}
