// Copyright (c) 2018 Colin Finck, RWTH Aachen University
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

const IRQ_FLAG_F: usize = 1 << 6;
const IRQ_FLAG_I: usize = 1 << 7;
const IRQ_FLAG_A: usize = 1 << 8;

/// Enable Interrupts
#[inline]
pub fn enable() {
	unsafe { asm!("msr daifclr, 0b111" ::: "memory" : "volatile"); }
}

/// Enable Interrupts and wait for the next interrupt (HLT instruction)
/// According to https://lists.freebsd.org/pipermail/freebsd-current/2004-June/029369.html, this exact sequence of assembly
/// instructions is guaranteed to be atomic.
/// This is important, because another CPU could call wakeup_core right when we decide to wait for the next interrupt.
#[inline]
pub fn enable_and_wait() {
	// TODO
	unsafe { asm!("msr daifclr, 0b111; wfi" :::: "volatile") };
}

/// Disable Interrupts
#[inline]
pub fn disable() {
	unsafe { asm!("msr daifset, 0b111" ::: "memory" : "volatile"); }
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
	unsafe { asm!("mrs $0, daif" : "=r"(flags) :: "memory" : "volatile"); }

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
pub extern "C" fn irq_install_handler(irq_number: u32, handler: usize)
{
	info!("Install handler for interrupt {}", irq_number);
	// TODO
}
