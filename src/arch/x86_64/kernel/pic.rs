use x86_64::instructions::port::Port;

use super::interrupts::IDT;
use crate::arch::x86_64::kernel::interrupts::ExceptionStackFrame;
use crate::arch::x86_64::swapgs;
use crate::scheduler;

const PIC1_COMMAND: Port<u8> = Port::new(0x20);
const PIC1_DATA: Port<u8> = Port::new(0x21);
const PIC2_COMMAND: Port<u8> = Port::new(0xa0);
const PIC2_DATA: Port<u8> = Port::new(0xa1);

pub const PIC1_INTERRUPT_OFFSET: u8 = 32;
const PIC2_INTERRUPT_OFFSET: u8 = 40;
const SPURIOUS_IRQ_NUMBER: u8 = 7;

/// End-Of-Interrupt Command for an Intel 8259 Programmable Interrupt Controller (PIC).
const PIC_EOI_COMMAND: u8 = 0x20;

pub fn eoi(int_no: u8) {
	let mut pic1_command = PIC1_COMMAND;
	let mut pic2_command = PIC2_COMMAND;

	unsafe {
		// For IRQ 8-15 (mapped to interrupt numbers >= 40), we need to send an EOI to the slave PIC.
		if int_no >= 40 {
			pic2_command.write(PIC_EOI_COMMAND);
		}

		// In all cases, we need to send an EOI to the master PIC.
		pic1_command.write(PIC_EOI_COMMAND);
	}
}

pub fn init() {
	let mut pic1_command = PIC1_COMMAND;
	let mut pic1_data = PIC1_DATA;
	let mut pic2_command = PIC2_COMMAND;
	let mut pic2_data = PIC2_DATA;

	// Even if we mask all interrupts, spurious interrupts may still occur.
	// This is especially true for real hardware. So provide a handler for them.
	unsafe {
		let mut idt = IDT.lock();
		idt[PIC1_INTERRUPT_OFFSET + SPURIOUS_IRQ_NUMBER]
			.set_handler_fn(spurious_interrupt_on_master)
			.set_stack_index(0);
		idt[PIC2_INTERRUPT_OFFSET + SPURIOUS_IRQ_NUMBER]
			.set_handler_fn(spurious_interrupt_on_slave)
			.set_stack_index(0);

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
		pic1_command.write(0x11);
		pic2_command.write(0x11);

		// Map PIC1 to interrupt numbers >= 32 and PIC2 to interrupt numbers >= 40.
		pic1_data.write(PIC1_INTERRUPT_OFFSET);
		pic2_data.write(PIC2_INTERRUPT_OFFSET);

		// Configure PIC1 as master and PIC2 as slave.
		pic1_data.write(0x04);
		pic2_data.write(0x02);

		// Start them in 8086 mode.
		pic1_data.write(0x01);
		pic2_data.write(0x01);

		// Mask all interrupts on both PICs.
		pic1_data.write(0xff);
		pic2_data.write(0xff);
	}
}

extern "x86-interrupt" fn spurious_interrupt_on_master(stack_frame: ExceptionStackFrame) {
	swapgs(&stack_frame);
	debug!("Spurious Interrupt on Master PIC (IRQ7)");
	scheduler::abort();
}

extern "x86-interrupt" fn spurious_interrupt_on_slave(stack_frame: ExceptionStackFrame) {
	swapgs(&stack_frame);
	debug!("Spurious Interrupt on Slave PIC (IRQ15)");

	// As this is an interrupt forwarded by the master, we have to acknowledge it on the master
	// (but not on the slave as with all spurious interrupts).
	let mut pic1_command = PIC1_COMMAND;
	unsafe {
		pic1_command.write(PIC_EOI_COMMAND);
	}
	scheduler::abort();
}

fn edit_mask(int_no: u8, insert: bool) {
	let mut port = if int_no >= 40 { PIC2_DATA } else { PIC1_DATA };
	let offset = if int_no >= 40 { 40 } else { 32 };

	unsafe {
		let mask = port.read();

		if insert {
			port.write(mask | (1 << (int_no - offset)));
		} else {
			port.write(mask & !(1 << (int_no - offset)));
		}
	}
}

pub fn mask(int_no: u8) {
	edit_mask(int_no, true);
}

pub fn unmask(int_no: u8) {
	edit_mask(int_no, false);
}
