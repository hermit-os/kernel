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
use arch::x86_64::percore::*;
use arch::x86_64::pic;
use arch::x86_64::processor;
use core::{fmt, mem};
use tasks::*;
use x86::shared::flags::*;


extern "C" {
	#[link_section = ".percore"]
	static __core_id: u32;

	#[link_section = ".percore"]
	static current_task: *const task_t;

	fn apic_is_enabled() -> i32;
	fn get_highest_priority() -> u32;
	fn scheduler() -> *const *const usize;

	// Defined in entry.asm using the irqstub macro.
	fn irq0();
	fn irq1();
	fn irq2();
	fn irq3();
	fn irq4();
	fn irq5();
	fn irq6();
	fn irq7();
	fn irq8();
	fn irq9();
	fn irq10();
	fn irq11();
	fn irq12();
	fn irq13();
	fn irq14();
	fn irq15();
	fn irq16();
	fn irq17();
	fn irq18();
	fn irq19();
	fn irq20();
	fn irq21();
	fn irq22();
	fn irq23();
	fn irq80();
	fn irq81();
	fn irq82();

	fn wakeup();
	fn mmnif_irq();
	fn apic_timer();
	fn apic_lint0();
	fn apic_lint1();
	fn apic_error();
	fn apic_svr();
}


static mut IRQ_HANDLERS: [usize; idt::IDT_ENTRIES] = [0; idt::IDT_ENTRIES];

/// This defines what the stack looks like after the task context is saved.
/// See also: arch/x86/include/asm/stddef.h
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

impl fmt::Display for state {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		let core_id = unsafe { __core_id.per_core() };
		let task = unsafe { current_task.per_core().as_ref().expect("current_task on core is NULL!") };

		writeln!(f, "Interrupt {} on core {} at {:#X}:{:#X}, fs = {:#X}, gs = {:#X}, rflags = {:#X}, error = {:#X}, task = {}", self.int_no, core_id, self.cs, self.rip, self.fs, self.gs, self.rflags, self.error, task.id).unwrap();
		write!(f, "rax = {:#X}, rbx = {:#X}, rcx = {:#X}, rdx = {:#X}, rbp = {:#X}, rsp = {:#X}, rdi = {:#X}, rsi = {:#X}, r8 = {:#X}, r9 = {:#X}, r10 = {:#X}, r11 = {:#X}, r12 = {:#X}, r13 = {:#X}, r14 = {:#X}, r15 = {:#X}",
			self.rax, self.rbx, self.rcx, self.rdx, self.rbp, self.rsp, self.rdi, self.rsi, self.r8, self.r9, self.r10, self.r11, self.r12, self.r13, self.r14, self.r15)
	}
}


#[inline]
pub fn set_handler(index: u8, handler: fn(&state)) {
	unsafe { IRQ_HANDLERS[index as usize] = handler as usize };
}

/// Enable Interrupts
#[inline]
pub fn enable() {
	unsafe { asm!("sti") };
}

/// Disable Interrupts
#[inline]
pub fn disable() {
	unsafe { asm!("cli") };
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

pub fn eoi(int_no: u64) {
	if unsafe { apic_is_enabled() } > 0 || int_no >= 48 {
		panic!("Called unimplemented APIC code path!");
	} else {
		pic::eoi(int_no as u8);
	}
}

pub fn install() {
	// Set gates to the IRQ stubs generated in entry.asm for the 24 IRQs.
	idt::set_gate(32, irq0, 1);
	idt::set_gate(33, irq1, 1);
	idt::set_gate(34, irq2, 1);
	idt::set_gate(35, irq3, 1);
	idt::set_gate(36, irq4, 1);
	idt::set_gate(37, irq5, 1);
	idt::set_gate(38, irq6, 1);
	idt::set_gate(39, irq7, 1);
	idt::set_gate(40, irq8, 1);
	idt::set_gate(41, irq9, 1);
	idt::set_gate(42, irq10, 1);
	idt::set_gate(43, irq11, 1);
	idt::set_gate(44, irq12, 1);
	idt::set_gate(45, irq13, 1);
	idt::set_gate(46, irq14, 1);
	idt::set_gate(47, irq15, 1);
	idt::set_gate(48, irq16, 1);
	idt::set_gate(49, irq17, 1);
	idt::set_gate(50, irq18, 1);
	idt::set_gate(51, irq19, 1);
	idt::set_gate(52, irq20, 1);
	idt::set_gate(53, irq21, 1);
	idt::set_gate(54, irq22, 1);
	idt::set_gate(55, irq23, 1);

	idt::set_gate(112, irq80, 1);
	idt::set_gate(113, irq81, 1);
	idt::set_gate(114, irq82, 1);

	idt::set_gate(121, wakeup, 1);
	idt::set_gate(122, mmnif_irq, 1);

	// Add APIC gates.
	idt::set_gate(123, apic_timer, 1);
	idt::set_gate(124, apic_lint0, 1);
	idt::set_gate(125, apic_lint1, 1);
	idt::set_gate(126, apic_error, 1);
	idt::set_gate(127, apic_svr, 1);
}


#[no_mangle]
pub extern "C" fn irq_handler(state_ptr: *const state) -> *const *const usize {
	let mut ret = 0 as *const *const usize;

	let state_ref = unsafe { state_ptr.as_ref().expect("state_ptr is NULL!") };
	assert!(state_ref.int_no < idt::IDT_ENTRIES as u64, "Got invalid IRQ {}", state_ref.int_no);

	let handler_address = unsafe { IRQ_HANDLERS[state_ref.int_no as usize] };
	assert!(handler_address > 0, "No handler installed for IRQ {}", state_ref.int_no);

	let handler = unsafe { mem::transmute::<usize, fn(&state)>(handler_address) };
	handler(state_ref);

	// Check that this is not the IRQ handler used during CPU initialization to measure the frequency.
	if processor::get_cpu_frequency() > 0 {
		unsafe {
			check_workqueues_in_irqhandler(state_ref.int_no as i32);

			let task = current_task.per_core().as_ref().expect("current_task on core is NULL!");
			if state_ref.int_no == 32 || state_ref.int_no == 123 {
				// This is a timer interrupt. Check if this unblocks any tasks.
				ret = scheduler();
			} else if state_ref.int_no >= 32 && get_highest_priority() > task.prio as u32 {
				// There is a ready task with a higher priority.
				ret = scheduler();
			}
		}
	}

	eoi(state_ref.int_no);
	ret
}
