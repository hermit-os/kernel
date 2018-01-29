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

use alloc::rc::Rc;
use arch::x86_64::apic;
use arch::x86_64::idt;
use arch::x86_64::irq;
use arch::x86_64::percore::*;
use arch::x86_64::processor;
use consts::*;
use core::cell::RefCell;
use core::{mem, ptr};
use scheduler;
use scheduler::task::{Task, TaskFrame, TaskTLS};

extern "C" {
	static tls_start: u8;
	static tls_end: u8;
}

#[repr(C, packed)]
pub struct State {
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
	/// status flags
	rflags: u64,
	/// instruction pointer
	rip: u64
}

extern "C" fn leave_task() -> ! {
	let core_scheduler = scheduler::get_scheduler(core_id());
	core_scheduler.exit(0);
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
		let core_scheduler = scheduler::get_scheduler(core_id());
		let task = core_scheduler.get_current_task();
		debug!("Set up TLS for task {} at address {:#X}", task.borrow().id, tls.address());
		task.borrow_mut().tls = Some(Rc::new(RefCell::new(tls)));
	}

	// Call the actual entry point of the task.
	func(arg);
}

impl TaskFrame for Task {
	fn create_stack_frame(&mut self, func: extern "C" fn(usize), arg: usize) {
		unsafe {
			let mut stack = ((*self.stack).top() - 16) as *mut u64;

			ptr::write_bytes((*self.stack).bottom() as *mut u8, 0xCD, KERNEL_STACK_SIZE);

			/* Only marker for debugging purposes, ... */
			*stack = 0xDEADBEEFu64;
			stack = (stack as usize - mem::size_of::<u64>()) as *mut u64;

			/*
			 * and the "caller" we shall return to.
			 * This procedure cleans the task after exit.
			 */
			*stack = (leave_task as *const()) as u64;
			stack = (stack as usize - mem::size_of::<State>()) as *mut u64;

			let state: *mut State = stack as *mut State;
			ptr::write_bytes(state as *mut u8, 0, mem::size_of::<State>());

			(*state).rsp = (stack as usize + mem::size_of::<State>()) as u64;
			(*state).rip = task_entry as u64;
			(*state).rdi = func as u64;
			(*state).rsi = arg as u64;
			(*state).rflags = 0x1202u64;

			/* Set the task's stack pointer entry to the stack we have crafted right now. */
			self.last_stack_pointer = stack as usize;
		}
	}
}

extern "x86-interrupt" fn timer_handler(stack_frame: &mut irq::ExceptionStackFrame) {
	let core_scheduler = scheduler::get_scheduler(core_id());
	core_scheduler.blocked_tasks.lock().handle_waiting_tasks();
	apic::eoi();
}

pub fn install_timer_handler() {
	idt::set_gate(apic::TIMER_INTERRUPT_NUMBER, timer_handler as usize, 1);
}
