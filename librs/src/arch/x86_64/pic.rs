// Copyright (c) 2017 Colin Finck, RWTH Aachen University
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
use arch::x86_64::irq::ExceptionStackFrame;
use x86::shared::io::*;

const PIC1_COMMAND_PORT: u16 = 0x20;
const PIC1_DATA_PORT:    u16 = 0x21;
const PIC2_COMMAND_PORT: u16 = 0xA0;
const PIC2_DATA_PORT:    u16 = 0xA1;

pub const PIC1_INTERRUPT_OFFSET: u8 = 32;
const PIC2_INTERRUPT_OFFSET: u8 = 40;
const SPURIOUS_IRQ_NUMBER:   u8 = 7;

/// End-Of-Interrupt Command for an Intel 8259 Programmable Interrupt Controller (PIC).
const PIC_EOI_COMMAND:   u8  = 0x20;


pub fn eoi(int_no: u8) {
	unsafe {
		// For IRQ 8-15 (mapped to interrupt numbers >= 40), we need to send an EOI to the slave PIC.
		if int_no >= 40 {
			outb(PIC2_COMMAND_PORT, PIC_EOI_COMMAND);
		}

		// In all cases, we need to send an EOI to the master PIC.
		outb(PIC1_COMMAND_PORT, PIC_EOI_COMMAND);
	}
}

pub fn init() {
	// Even if we mask all interrupts, spurious interrupts may still occur.
	// This is especially true for real hardware. So provide a handler for them.
	idt::set_gate(PIC1_INTERRUPT_OFFSET + SPURIOUS_IRQ_NUMBER, spurious_interrupt_on_master as usize, 1);
	idt::set_gate(PIC2_INTERRUPT_OFFSET + SPURIOUS_IRQ_NUMBER, spurious_interrupt_on_slave as usize, 1);

	unsafe {
		// Reinitialize PIC1 and PIC2.
		outb(PIC1_COMMAND_PORT, 0x11);
		outb(PIC2_COMMAND_PORT, 0x11);

		// Map PIC1 to interrupt numbers >= 32 and PIC2 to interrupt numbers >= 40.
		outb(PIC1_DATA_PORT, PIC1_INTERRUPT_OFFSET);
		outb(PIC2_DATA_PORT, PIC2_INTERRUPT_OFFSET);

		// Configure PIC1 as master and PIC2 as slave.
		outb(PIC1_DATA_PORT, 0x04);
		outb(PIC2_DATA_PORT, 0x02);

		// Start them in 8086 mode.
		outb(PIC1_DATA_PORT, 0x01);
		outb(PIC2_DATA_PORT, 0x01);

		// Mask all interrupts on both PICs.
		outb(PIC1_DATA_PORT, 0xFF);
		outb(PIC2_DATA_PORT, 0xFF);
	}
}

extern "x86-interrupt" fn spurious_interrupt_on_master(stack_frame: &mut ExceptionStackFrame) {
	debug!("Spurious Interrupt on Master PIC (IRQ7)");
}

extern "x86-interrupt" fn spurious_interrupt_on_slave(stack_frame: &mut ExceptionStackFrame) {
	debug!("Spurious Interrupt on Slave PIC (IRQ15)");

	// As this is an interrupt forwarded by the master, we have to acknowledge it on the master
	// (but not on the slave as with all spurious interrupts).
	unsafe { outb(PIC1_COMMAND_PORT, PIC_EOI_COMMAND); }
}

fn edit_mask(int_no: u8, insert: bool) {
	let port = if int_no >= 40 { PIC2_DATA_PORT } else { PIC1_DATA_PORT };
	let offset = if int_no >= 40 { 40 } else { 32 };

	unsafe {
		let mask = inb(port);

		if insert {
			outb(port, mask | 1 << (int_no - offset));
		} else {
			outb(port, mask & !(1 << (int_no - offset)));
		}
	}
}

pub fn mask(int_no: u8) {
	edit_mask(int_no, true);
}

pub fn unmask(int_no: u8) {
	edit_mask(int_no, false);
}
