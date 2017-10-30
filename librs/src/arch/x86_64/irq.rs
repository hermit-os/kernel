// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
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

/// Enable Interrupts
#[inline(always)]
pub fn irq_enable() {
	unsafe { asm!("sti" ::: "memory" : "volatile") };
}

/// Disable Interrupts
#[inline(always)]
pub fn irq_disable() {
	unsafe { asm!("cli" ::: "memory" : "volatile") };
}

/// Determines, if the interrupt flags (IF) is set
#[inline(always)]
pub fn is_irq_enabled() -> bool
{
	let rflags: u64;

	unsafe { asm!("pushf; pop $0": "=r"(rflags) :: "memory" : "volatile") };
	if (rflags & (1u64 << 9)) !=  0 {
		return true;
	}

	false
}

/// Disable IRQs (nested)
///
/// Disable IRQs when unsure if IRQs were enabled at all.
/// This function together with irq_nested_enable can be used
/// in situations when interrupts shouldn't be activated if they
/// were not activated before calling this function.
#[inline(always)]
pub fn irq_nested_disable() -> bool {
	let was_enabled = is_irq_enabled();
	irq_disable();
	was_enabled
}

/// Enable IRQs (nested)
///
/// Can be used in conjunction with irq_nested_disable() to only enable
/// interrupts again if they were enabled before.
#[inline(always)]
pub fn irq_nested_enable(was_enabled: bool) {
	if was_enabled == true {
		irq_enable();
	}
}
