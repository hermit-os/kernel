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

use arch::x86_64::idt;
use arch::x86_64::irq;
use logging::*;
use x86::shared::irq::EXCEPTIONS;

extern "C" {
	fn fpu_handler();

	// Defined in entry.asm using the isrstub_pseudo_error macro.
	fn isr0();
	fn isr1();
	fn isr2();
	fn isr3();
	fn isr4();
	fn isr5();
	fn isr6();
	fn isr7();
	fn isr8();
	fn isr9();
	fn isr10();
	fn isr11();
	fn isr12();
	fn isr13();
	fn isr14();
	fn isr15();
	fn isr16();
	fn isr17();
	fn isr18();
	fn isr19();
	fn isr20();
	fn isr21();
	fn isr22();
	fn isr23();
	fn isr24();
	fn isr25();
	fn isr26();
	fn isr27();
	fn isr28();
	fn isr29();
	fn isr30();
	fn isr31();
}

pub fn install() {
	// Set gates to the Interrupt Service Routines (ISRs) generated in entry.asm for all 32 CPU exceptions.
	// All of them use dedicated stacks (Interrupt Stack Tables = ISTs) to prevent clobbering the current task stack.
	// Some very critical exceptions even get their own stacks to always execute on a known good stack:
	//   - Non-Maskable Interrupt Exception (IST2)
	//   - Double Fault Exception (IST3)
	//   - Machine Check Exception (IST4)
	//
	// Refer to Intel Vol. 3A, 6.14.5 Interrupt Stack Table.
	idt::set_gate(0, isr0, 1);
	idt::set_gate(1, isr1, 1);
	idt::set_gate(2, isr2, 2);
	idt::set_gate(3, isr3, 1);
	idt::set_gate(4, isr4, 1);
	idt::set_gate(5, isr5, 1);
	idt::set_gate(6, isr6, 1);
	idt::set_gate(7, isr7, 1);
	idt::set_gate(8, isr8, 3);
	idt::set_gate(9, isr9, 1);
	idt::set_gate(10, isr10, 1);
	idt::set_gate(11, isr11, 1);
	idt::set_gate(12, isr12, 1);
	idt::set_gate(13, isr13, 1);
	idt::set_gate(14, isr14, 1);
	idt::set_gate(15, isr15, 1);
	idt::set_gate(16, isr16, 1);
	idt::set_gate(17, isr17, 1);
	idt::set_gate(18, isr18, 4);
	idt::set_gate(19, isr19, 1);
	idt::set_gate(20, isr20, 1);
	idt::set_gate(21, isr21, 1);
	idt::set_gate(22, isr22, 1);
	idt::set_gate(23, isr23, 1);
	idt::set_gate(24, isr24, 1);
	idt::set_gate(25, isr25, 1);
	idt::set_gate(26, isr26, 1);
	idt::set_gate(27, isr27, 1);
	idt::set_gate(28, isr28, 1);
	idt::set_gate(29, isr29, 1);
	idt::set_gate(30, isr30, 1);
	idt::set_gate(31, isr31, 1);

	// Output a message for each of them using a common exception handler.
	for i in 0..32 {
		irq::set_handler(i, exception_handler);
	}

	// The FPU Exception 7 indicates that the FPU is being used after a task switch and needs to be handled.
	irq::set_handler(7, x86_fpu_handler);
}

fn exception_handler(state_ref: &irq::state) {
	error!("{} Exception, {}", if state_ref.int_no < EXCEPTIONS.len() as u64 { EXCEPTIONS[state_ref.int_no as usize].description } else { "Unknown" }, state_ref);
	irq::eoi(state_ref.int_no);
	panic!();
}

fn x86_fpu_handler(state_ref: &irq::state) {
	unsafe {
		asm!("clts");
		fpu_handler();
	}
}
