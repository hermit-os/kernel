// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//               2018 Colin Finck, RWTH Aachen University
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

//! Architecture dependent interface to initialize a task

include!(concat!(env!("CARGO_TARGET_DIR"), "/config.rs"));

use alloc::rc::Rc;
use arch::x86_64::apic;
use arch::x86_64::idt;
use arch::x86_64::irq;
use arch::x86_64::percore::*;
use arch::x86_64::processor;
use core::cell::RefCell;
use core::{mem, ptr};
use scheduler::task::{Task, TaskFrame, TaskTLS};

extern "C" {
	static tls_start: u8;
	static tls_end: u8;
}

#[repr(C, packed)]
struct State {
	/// FS register for TLS support
	fs: u64,
	/// R15 register
	r15: u64,
	/// R14 register
	r14: u64,
	/// R13 register
	r13: u64,
	/// R12 register
	r12: u64,
	/// R11 register
	r11: u64,
	/// R10 register
	r10: u64,
	/// R9 register
	r9: u64,
	/// R8 register
	r8: u64,
	/// RDI register
	rdi: u64,
	/// RSI register
	rsi: u64,
	/// RBP register
	rbp: u64,
	/// RBX register
	rbx: u64,
	/// RDX register
	rdx: u64,
	/// RCX register
	rcx: u64,
	/// RAX register
	rax: u64,
	/// status flags
	rflags: u64,
	/// instruction pointer
	rip: u64
}

extern "C" fn leave_task() -> ! {
	core_scheduler().exit(0);
}

extern "C" fn task_entry(func: extern "C" fn(usize), arg: usize) {
	// Check if the task (process or thread) uses Thread-Local-Storage.
	let tls_size = unsafe { &tls_end as *const u8 as usize - &tls_start as *const u8 as usize };
	if tls_size > 0 {
		// Allocate TLS memory, copy over the TLS variables, and set the FS register accordingly.
		let tls = TaskTLS::new(tls_size);
		unsafe { ptr::copy_nonoverlapping(&tls_start as *const u8, tls.address() as *mut u8, tls_size); }
		processor::writefs(tls.address() + tls_size);

		// Associate the TLS memory to the current task.
		let mut current_task_borrowed = core_scheduler().current_task.borrow_mut();
		debug!("Set up TLS for task {} at address {:#X}", current_task_borrowed.id, tls.address());
		current_task_borrowed.tls = Some(Rc::new(RefCell::new(tls)));
	}

	// Call the actual entry point of the task.
	func(arg);
}

impl TaskFrame for Task {
	fn create_stack_frame(&mut self, func: extern "C" fn(usize), arg: usize) {
		unsafe {
			// Mark the entire stack with 0xCD.
			ptr::write_bytes(self.stack as *mut u8, 0xCD, DEFAULT_STACK_SIZE);

			// Set a marker for debugging at the very top.
			let mut stack = (self.stack + DEFAULT_STACK_SIZE - 0x10) as *mut u64;
			*stack = 0xDEADBEEFu64;

			// Put the leave_task function on the stack.
			// When the task has finished, it will call this function by returning.
			stack = (stack as usize - mem::size_of::<u64>()) as *mut u64;
			*stack = leave_task as u64;

			// Put the State structure expected by the ASM switch() function on the stack.
			stack = (stack as usize - mem::size_of::<State>()) as *mut u64;

			let state = stack as *mut State;
			ptr::write_bytes(state as *mut u8, 0, mem::size_of::<State>());

			(*state).rip = task_entry as u64;
			(*state).rdi = func as u64;
			(*state).rsi = arg as u64;
			(*state).rflags = 0x1202u64;

			// Set the task's stack pointer entry to the stack we have just crafted.
			self.last_stack_pointer = stack as usize;
		}
	}
}

extern "x86-interrupt" fn timer_handler(_stack_frame: &mut irq::ExceptionStackFrame) {
	core_scheduler().blocked_tasks.lock().handle_waiting_tasks();
	apic::eoi();
}

pub fn install_timer_handler() {
	idt::set_gate(apic::TIMER_INTERRUPT_NUMBER, timer_handler as usize, 1);
}
