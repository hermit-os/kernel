// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

const IRQ_FLAG_F: usize = 1 << 6;
const IRQ_FLAG_I: usize = 1 << 7;
const IRQ_FLAG_A: usize = 1 << 8;

/// Enable Interrupts
#[inline]
pub fn enable() {
	unsafe {
		llvm_asm!("msr daifclr, 0b111" ::: "memory" : "volatile");
	}
}

/// Enable Interrupts and wait for the next interrupt (HLT instruction)
/// According to https://lists.freebsd.org/pipermail/freebsd-current/2004-June/029369.html, this exact sequence of assembly
/// instructions is guaranteed to be atomic.
/// This is important, because another CPU could call wakeup_core right when we decide to wait for the next interrupt.
#[inline]
pub fn enable_and_wait() {
	// TODO
	unsafe { llvm_asm!("msr daifclr, 0b111; wfi" :::: "volatile") };
}

/// Disable Interrupts
#[inline]
pub fn disable() {
	unsafe {
		llvm_asm!("msr daifset, 0b111" ::: "memory" : "volatile");
	}
}

/// Disable IRQs (nested)
///
/// Disable IRQs when unsure if IRQs were enabled at all.
/// This function together with nested_enable can be used
/// in situations when interrupts shouldn't be activated if they
/// were not activated before calling this function.
#[inline]
pub fn nested_disable() -> bool {
	let flags: usize;
	unsafe {
		llvm_asm!("mrs $0, daif" : "=r"(flags) :: "memory" : "volatile");
	}

	let mut was_enabled = true;
	if flags & (IRQ_FLAG_A | IRQ_FLAG_I | IRQ_FLAG_F) > 0 {
		was_enabled = false;
	}

	disable();
	was_enabled
}

/// Enable IRQs (nested)
///
/// Can be used in conjunction with nested_disable() to only enable
/// interrupts again if they were enabled before.
#[inline]
pub fn nested_enable(was_enabled: bool) {
	if was_enabled {
		enable();
	}
}

#[no_mangle]
pub extern "C" fn irq_install_handler(irq_number: u32, handler: usize) {
	info!("Install handler for interrupt {}", irq_number);
	// TODO
}
