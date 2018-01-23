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
use arch::x86_64::mm::paging;
use core::fmt;
use scheduler;
use x86::shared::flags::*;


// Derived from Philipp Oppermann's blog
// => https://github.com/phil-opp/blog_os/blob/master/src/interrupts/mod.rs
/// Represents the exception stack frame pushed by the CPU on exception entry.
#[repr(C)]
pub struct ExceptionStackFrame {
    /// This value points to the instruction that should be executed when the interrupt
    /// handler returns. For most interrupts, this value points to the instruction immediately
    /// following the last executed instruction. However, for some exceptions (e.g., page faults),
    /// this value points to the faulting instruction, so that the instruction is restarted on
    /// return. See the documentation of the `Idt` fields for more details.
    pub instruction_pointer: u64,
    /// The code segment selector, padded with zeros.
    pub code_segment: u64,
    /// The flags register before the interrupt handler was invoked.
    pub cpu_flags: u64,
    /// The stack pointer at the time of the interrupt.
    pub stack_pointer: u64,
    /// The stack segment descriptor at the time of the interrupt (often zero in 64-bit mode).
    pub stack_segment: u64,
}

impl fmt::Debug for ExceptionStackFrame {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        struct Hex(u64);
        impl fmt::Debug for Hex {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "{:#x}", self.0)
            }
        }

        let mut s = f.debug_struct("ExceptionStackFrame");
        s.field("instruction_pointer", &Hex(self.instruction_pointer));
        s.field("code_segment", &Hex(self.code_segment));
        s.field("cpu_flags", &Hex(self.cpu_flags));
        s.field("stack_pointer", &Hex(self.stack_pointer));
        s.field("stack_segment", &Hex(self.stack_segment));
        s.finish()
    }
}


/// Enable Interrupts
#[inline]
pub fn enable() {
	unsafe { asm!("sti" :::: "volatile") };
}

/// Disable Interrupts
#[inline]
pub fn disable() {
	unsafe { asm!("cli" :::: "volatile") };
}

/// Disable IRQs (nested)
///
/// Disable IRQs when unsure if IRQs were enabled at all.
/// This function together with nested_enable can be used
/// in situations when interrupts shouldn't be activated if they
/// were not activated before calling this function.
#[inline]
pub fn nested_disable() -> bool {
	let was_enabled = flags().contains(FLAGS_IF);
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

pub fn install() {
	// Set gates to the Interrupt Service Routines (ISRs) for all 32 CPU exceptions.
	// All of them use a dedicated stack per task (IST1) to prevent clobbering the current task stack.
	// Some critical exceptions also get their own stacks to always execute on a known good stack:
	//   - Non-Maskable Interrupt Exception (IST2)
	//   - Double Fault Exception (IST3)
	//   - Machine Check Exception (IST4)
	//
	// Refer to Intel Vol. 3A, 6.14.5 Interrupt Stack Table.
	idt::set_gate(0, divide_error_exception as usize, 1);
	idt::set_gate(1, debug_exception as usize, 1);
	idt::set_gate(2, nmi_exception as usize, 2);
	idt::set_gate(3, breakpoint_exception as usize, 1);
	idt::set_gate(4, overflow_exception as usize, 1);
	idt::set_gate(5, bound_range_exceeded_exception as usize, 1);
	idt::set_gate(6, invalid_opcode_exception as usize, 1);
	idt::set_gate(7, device_not_available_exception as usize, 1);
	idt::set_gate(8, double_fault_exception as usize, 3);
	idt::set_gate(9, coprocessor_segment_overrun_exception as usize, 1);
	idt::set_gate(10, invalid_tss_exception as usize, 1);
	idt::set_gate(11, segment_not_present_exception as usize, 1);
	idt::set_gate(12, stack_segment_fault_exception as usize, 1);
	idt::set_gate(13, general_protection_exception as usize, 1);
	idt::set_gate(14, paging::page_fault_handler as usize, 1);
	idt::set_gate(15, reserved_exception as usize, 1);
	idt::set_gate(16, floating_point_exception as usize, 1);
	idt::set_gate(17, alignment_check_exception as usize, 1);
	idt::set_gate(18, machine_check_exception as usize, 4);
	idt::set_gate(19, simd_floating_point_exception as usize, 1);
	idt::set_gate(20, virtualization_exception as usize, 1);
	idt::set_gate(21, reserved_exception as usize, 1);
	idt::set_gate(22, reserved_exception as usize, 1);
	idt::set_gate(23, reserved_exception as usize, 1);
	idt::set_gate(24, reserved_exception as usize, 1);
	idt::set_gate(25, reserved_exception as usize, 1);
	idt::set_gate(26, reserved_exception as usize, 1);
	idt::set_gate(27, reserved_exception as usize, 1);
	idt::set_gate(28, reserved_exception as usize, 1);
	idt::set_gate(29, reserved_exception as usize, 1);
	idt::set_gate(30, reserved_exception as usize, 1);
	idt::set_gate(31, reserved_exception as usize, 1);
}


extern "x86-interrupt" fn divide_error_exception(stack_frame: &mut ExceptionStackFrame) {
	error!("Divide Error (#DE) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn debug_exception(stack_frame: &mut ExceptionStackFrame) {
	error!("Debug (#DB) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn nmi_exception(stack_frame: &mut ExceptionStackFrame) {
	error!("Non-Maskable Interrupt (NMI) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn breakpoint_exception(stack_frame: &mut ExceptionStackFrame) {
	error!("Breakpoint (#BP) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn overflow_exception(stack_frame: &mut ExceptionStackFrame) {
	error!("Overflow (#OF) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn bound_range_exceeded_exception(stack_frame: &mut ExceptionStackFrame) {
	error!("BOUND Range Exceeded (#BR) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn invalid_opcode_exception(stack_frame: &mut ExceptionStackFrame) {
	error!("Invalid Opcode (#UD) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn device_not_available_exception(_stack_frame: &mut ExceptionStackFrame) {
	// We set the CR0_TASK_SWITCHED flag every time we switch to a task.
	// This causes the "Device Not Available" Exception (int #7) to be thrown as soon as we use the FPU for the first time.
	// We have to clear the CR0_TASK_SWITCHED here and save the FPU context of the old task.

	unsafe { asm!("clts" :::: "volatile"); }
	panic!("FPU ToDo");
}

extern "x86-interrupt" fn double_fault_exception(stack_frame: &mut ExceptionStackFrame, error_code: u64) {
	error!("Double Fault (#DF) Exception: {:#?}, error {:#X}", stack_frame, error_code);
	scheduler::abort();
}

extern "x86-interrupt" fn coprocessor_segment_overrun_exception(stack_frame: &mut ExceptionStackFrame) {
	error!("CoProcessor Segment Overrun (#MF) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn invalid_tss_exception(stack_frame: &mut ExceptionStackFrame) {
	error!("Invalid TSS (#TS) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn segment_not_present_exception(stack_frame: &mut ExceptionStackFrame) {
	error!("Segment Not Present (#NP) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn stack_segment_fault_exception(stack_frame: &mut ExceptionStackFrame, error_code: u64) {
	error!("Stack Segment Fault (#SS) Exception: {:#?}, error {:#X}", stack_frame, error_code);
	scheduler::abort();
}

extern "x86-interrupt" fn general_protection_exception(stack_frame: &mut ExceptionStackFrame, error_code: u64) {
	error!("General Protection (#GP) Exception: {:#?}, error {:#X}", stack_frame, error_code);
	scheduler::abort();
}

extern "x86-interrupt" fn floating_point_exception(stack_frame: &mut ExceptionStackFrame) {
	error!("Floating-Point Error (#MF) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn alignment_check_exception(stack_frame: &mut ExceptionStackFrame) {
	error!("Alignment Check (#AC) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn machine_check_exception(stack_frame: &mut ExceptionStackFrame) {
	error!("Machine Check (#MC) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn simd_floating_point_exception(stack_frame: &mut ExceptionStackFrame) {
	error!("SIMD Floating-Point (#XM) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn virtualization_exception(stack_frame: &mut ExceptionStackFrame) {
	error!("Virtualization (#VE) Exception: {:#?}", stack_frame);
	scheduler::abort();
}

extern "x86-interrupt" fn reserved_exception(stack_frame: &mut ExceptionStackFrame) {
	error!("Reserved Exception: {:#?}", stack_frame);
	scheduler::abort();
}
