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

use logging::*;

// MODULES
pub mod gdt;
pub mod idt;
pub mod irq;
pub mod isr;
pub mod mm;
pub mod percore;
pub mod pic;
pub mod pit;
pub mod processor;

extern "C" {
	fn memory_init() -> i32;
	fn signal_init();
}

// FUNCTIONS
pub fn system_init() {
	gdt::install();
	idt::install();
	processor::detect_features();
	processor::configure();
	mm::paging::map_boot_info();
	pic::remap();
	pit::deinit();
	isr::install();
	irq::install();
	irq::enable();
	processor::detect_frequency();
	processor::print_information();

	loop {
		info!("Moin");
		unsafe { processor::udelay(1_000_000); }
	}

	unsafe {
		memory_init();
		signal_init();
	}
}
