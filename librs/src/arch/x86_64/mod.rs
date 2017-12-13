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
pub mod serial;
pub mod vga;

use arch::x86_64::serial::SerialPort;
use synch::spinlock::Spinlock;


const SERIAL_PORT_ADDRESS: u16 = 0x3F8;
const SERIAL_PORT_BAUDRATE: u32 = 115200;


extern "C" {
	static mut cpu_online: u32;
}

lazy_static! {
	static ref CPU_ONLINE: Spinlock<&'static mut u32> =
		Spinlock::new(unsafe { &mut cpu_online });
}

static COM1: SerialPort = SerialPort::new(SERIAL_PORT_ADDRESS);


// FUNCTIONS
pub fn message_output_init() {
	COM1.init(SERIAL_PORT_BAUDRATE);
}

pub fn output_message_byte(byte: u8) {
	COM1.write_byte(byte);
	vga::write_byte(byte);
}

pub fn boot_processor_init() {
	percore::init();
	gdt::install();
	idt::install();
	processor::detect_features();
	processor::configure();
	vga::init();
	::mm::init();
	::mm::print_information();
	pic::init();
	irq::install();
	irq::enable();
	processor::detect_frequency();
	processor::print_information();
	pci::init();
	pci::print_information();

	**CPU_ONLINE.lock() += 1;

	apic::init();
	apic::print_information();

	loop {
		info!("Moin");
		processor::udelay(5_000_000);
	}

	/*unsafe {
		signal_init();
	}*/
}

pub fn application_processor_init() {
	percore::init();
	gdt::install();
	idt::install();
	processor::configure();
	apic::init_x2apic();
	apic::init_local_apic();
	irq::enable();

	debug!("Initialized Application Processor");
	**CPU_ONLINE.lock() += 1;
}
