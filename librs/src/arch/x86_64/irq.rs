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

/// This defines what the stack looks like after the task context is saved.
/// See also: arch/x86/include/asm/stddef.h
///
/// TODO: This interface may be overhauled as soon as all IRQ handlers have been ported to Rust.
#[repr(C)]
pub struct state {
	/// GS register
	pub gs: u64,
	/// FS register for TLS support
	pub fs: u64,
	/// R15 register
	pub r15: u64,
	/// R14 register
	pub r14: u64,
	/// R13 register
	pub r13: u64,
	/// R12 register
	pub r12: u64,
	/// R11 register
	pub r11: u64,
	/// R10 register
	pub r10: u64,
	/// R9 register
	pub r9: u64,
	/// R8 register
	pub r8: u64,
	/// RDI register
	pub rdi: u64,
	/// RSI register
	pub rsi: u64,
	/// RBP register
	pub rbp: u64,
	/// (pseudo) RSP register
	pub rsp: u64,
	/// RBX register
	pub rbx: u64,
	/// RDX register
	pub rdx: u64,
	/// RCX register
	pub rcx: u64,
	/// RAX register
	pub rax: u64,

	/// Interrupt number
	pub int_no: u64,

	/// pushed by the processor automatically
	pub error: u64,
	pub rip: u64,
	pub cs: u64,
	pub rflags: u64,
	pub userrsp: u64,
	pub ss: u64,
}

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
