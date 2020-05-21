// Copyright (c) 2017 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::arch::x86_64::kernel::idt;
use crate::arch::x86_64::kernel::irq::ExceptionStackFrame;
use crate::x86::io::*;

const PIC1_COMMAND_PORT: u16 = 0x20;
const PIC1_DATA_PORT: u16 = 0x21;
const PIC2_COMMAND_PORT: u16 = 0xA0;
const PIC2_DATA_PORT: u16 = 0xA1;

pub const PIC1_INTERRUPT_OFFSET: u8 = 32;
const PIC2_INTERRUPT_OFFSET: u8 = 40;
const SPURIOUS_IRQ_NUMBER: u8 = 7;

/// End-Of-Interrupt Command for an Intel 8259 Programmable Interrupt Controller (PIC).
const PIC_EOI_COMMAND: u8 = 0x20;

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
	idt::set_gate(
		PIC1_INTERRUPT_OFFSET + SPURIOUS_IRQ_NUMBER,
		spurious_interrupt_on_master as usize,
		0,
	);
	idt::set_gate(
		PIC2_INTERRUPT_OFFSET + SPURIOUS_IRQ_NUMBER,
		spurious_interrupt_on_slave as usize,
		0,
	);

	unsafe {
		// Remapping IRQs with a couple of IO output operations
		//
		// Normally, IRQs 0 to 7 are mapped to entries 8 to 15. This
		// is a problem in protected mode, because IDT entry 8 is a
		// Double Fault! Without remapping, every time IRQ0 fires,
		// you get a Double Fault Exception, which is NOT what's
		// actually happening. We send commands to the Programmable
		// Interrupt Controller (PICs - also called the 8259's) in
		// order to make IRQ0 to 15 be remapped to IDT entries 32 to
		// 47

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

extern "x86-interrupt" fn spurious_interrupt_on_master(_stack_frame: &mut ExceptionStackFrame) {
	debug!("Spurious Interrupt on Master PIC (IRQ7)");
}

extern "x86-interrupt" fn spurious_interrupt_on_slave(_stack_frame: &mut ExceptionStackFrame) {
	debug!("Spurious Interrupt on Slave PIC (IRQ15)");

	// As this is an interrupt forwarded by the master, we have to acknowledge it on the master
	// (but not on the slave as with all spurious interrupts).
	unsafe {
		outb(PIC1_COMMAND_PORT, PIC_EOI_COMMAND);
	}
}

fn edit_mask(int_no: u8, insert: bool) {
	let port = if int_no >= 40 {
		PIC2_DATA_PORT
	} else {
		PIC1_DATA_PORT
	};
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
