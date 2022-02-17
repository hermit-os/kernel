use core::arch::asm;

const IRQ_FLAG_F: usize = 1 << 6;
const IRQ_FLAG_I: usize = 1 << 7;
const IRQ_FLAG_A: usize = 1 << 8;

/// Enable Interrupts
#[inline]
pub fn enable() {
	unsafe {
		asm!(
			"msr daifclr, {mask}",
			mask = const 0b111,
			options(nostack, nomem),
		);
	}
}

/// Enable Interrupts and wait for the next interrupt (HLT instruction)
/// According to <https://lists.freebsd.org/pipermail/freebsd-current/2004-June/029369.html>, this exact sequence of assembly
/// instructions is guaranteed to be atomic.
/// This is important, because another CPU could call wakeup_core right when we decide to wait for the next interrupt.
#[inline]
pub fn enable_and_wait() {
	unsafe {
		asm!(
			"msr daifclr, {mask}; wfi",
			mask = const 0b111,
			options(nostack, nomem),
		);
	}
}

/// Disable Interrupts
#[inline]
pub fn disable() {
	unsafe {
		asm!(
			"msr daifset, {mask}",
			mask = const 0b111,
			options(nostack, nomem),
		);
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
		asm!(
			"mrs {}, daif",
			out(reg) flags,
			options(nostack, nomem),
		);
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

#[no_mangle]
pub extern "C" fn do_fiq(_: *const u8) {
	debug!("Receive fast interrupt\n");

	loop {
		crate::arch::processor::halt()
	}
}

#[no_mangle]
pub extern "C" fn do_irq(_: *const u8) {
	debug!("Receive interrupt\n");

	loop {
		crate::arch::processor::halt()
	}
}

#[no_mangle]
pub extern "C" fn do_sync(_: *const u8) {
	debug!("Receive synchronous exception\n");

	loop {
		crate::arch::processor::halt()
	}
}

#[no_mangle]
pub extern "C" fn do_bad_mode(_: *const u8, reason: u32) {
	error!("Receive unhandled exception: {}\n", reason);

	loop {
		crate::arch::processor::halt()
	}
}

#[no_mangle]
pub extern "C" fn do_error(_: *const u8) {
	error!("Receive error interrupt\n");

	loop {
		crate::arch::processor::halt()
	}
}
