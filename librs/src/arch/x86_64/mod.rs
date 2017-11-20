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

pub mod gdt;
pub mod idt;
pub mod irq;
pub mod isr;
pub mod mm;
pub mod percore;
pub mod pic;
pub mod pit;
pub mod processor;

use logging::*;


extern "C" {
	static image_size: usize;
	static kernel_start: u8;

	fn memory_init() -> i32;
	fn signal_init();
}


// FUNCTIONS
pub fn system_init() {
	gdt::install();
	idt::install();
	processor::detect_features();
	processor::configure();
	::mm::init();
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
		signal_init();
	}
}
