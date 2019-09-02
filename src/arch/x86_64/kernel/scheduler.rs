// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//               2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! Architecture dependent interface to initialize a task

use alloc::rc::Rc;
use arch::x86_64::kernel::apic;
use arch::x86_64::kernel::gdt;
use arch::x86_64::kernel::idt;
use arch::x86_64::kernel::irq;
use arch::x86_64::kernel::percore::*;
use arch::x86_64::kernel::processor;
use config::*;
use core::cell::RefCell;
use core::{mem, ptr};
use scheduler::task::{Task, TaskFrame, TaskTLS};

#[cfg(not(test))]
extern "C" {
	static tls_start: usize;
	static tls_end: usize;
	static tdata_end: usize;
}

#[repr(C, packed)]
struct State {
	/// GS register
	gs: usize,
	/// FS register for TLS support
	fs: usize,
	/// R15 register
	r15: usize,
	/// R14 register
	r14: usize,
	/// R13 register
	r13: usize,
	/// R12 register
	r12: usize,
	/// R11 register
	r11: usize,
	/// R10 register
	r10: usize,
	/// R9 register
	r9: usize,
	/// R8 register
	r8: usize,
	/// RDI register
	rdi: usize,
	/// RSI register
	rsi: usize,
	/// RBP register
	rbp: usize,
	/// RBX register
	rbx: usize,
	/// RDX register
	rdx: usize,
	/// RCX register
	rcx: usize,
	/// RAX register
	rax: usize,
	/// status flags
	rflags: usize,
	/// instruction pointer
	rip: usize,
}

#[derive(Default)]
pub struct TaskStacks {
	/// Whether this is a boot stack
	is_boot_stack: bool,
	/// Stack of the task
	pub stack: usize,
}

impl TaskStacks {
	pub fn new() -> Self {
		// Allocate an executable stack to possibly support dynamically generated code on the stack (see https://security.stackexchange.com/a/47825).
		let stack = ::mm::allocate(DEFAULT_STACK_SIZE, false);
		debug!("Allocating stack {:#X}", stack);

		Self {
			is_boot_stack: false,
			stack: stack,
		}
	}

	pub fn from_boot_stacks() -> Self {
		let stack = gdt::get_boot_stacks();
		debug!("Using boot stack {:#X}", stack);

		Self {
			is_boot_stack: true,
			stack: stack,
		}
	}
}

impl Drop for TaskStacks {
	fn drop(&mut self) {
		if !self.is_boot_stack {
			debug!("Deallocating stack {:#X}", self.stack);

			::mm::deallocate(self.stack, DEFAULT_STACK_SIZE);
		}
	}
}

extern "C" fn leave_task() -> ! {
	core_scheduler().exit(0);
}

#[cfg(test)]
extern "C" fn task_entry(func: extern "C" fn(usize), arg: usize) {}

#[cfg(not(test))]
extern "C" fn task_entry(func: extern "C" fn(usize), arg: usize) {
	// determine the size of tdata (tls without tbss)
	let tdata_size: usize =
		unsafe { &tdata_end as *const usize as usize - &tls_start as *const usize as usize };

	// Check if the task (process or thread) uses Thread-Local-Storage.
	let tls_size =
		unsafe { &tls_end as *const usize as usize - &tls_start as *const usize as usize };
	if tls_size > 0 {
		// Yes, it does, so we have to allocate TLS memory.
		// Allocate enough space for the given size and one more variable of type usize, which holds the tls_pointer.
		let tls_allocation_size = align_up!(tls_size, 32) + mem::size_of::<usize>();
		let tls = TaskTLS::new(tls_allocation_size);

		// The tls_pointer is the address to the end of the TLS area requested by the task.
		let tls_pointer = tls.address() + align_up!(tls_size, 32);

		// As per the x86-64 TLS specification, the FS register holds the tls_pointer.
		// This allows TLS variable values to be accessed by "mov rax, fs:VARIABLE_OFFSET".
		processor::writefs(tls_pointer);
		debug!(
			"Set FS to 0x{:x}, TLS size 0x{:x}, TLS data size 0x{:x}",
			tls_pointer, tls_size, tdata_size
		);

		unsafe {
			// The x86-64 TLS specification also requires that the tls_pointer can be accessed at fs:0.
			// This allows TLS variable values to be accessed by "mov rax, fs:0" and a later "lea rdx, [rax+VARIABLE_OFFSET]".
			// See "ELF Handling For Thread-Local Storage", version 0.20 by Ulrich Drepper, page 12 for details.
			//
			// fs:0 is where tls_pointer points to and we have reserved space for a usize value above.
			*(tls_pointer as *mut usize) = tls_pointer;

			// Copy over TLS variables with their initial values.
			ptr::copy_nonoverlapping(
				&tls_start as *const usize as *const u8,
				tls.address() as *mut u8,
				tdata_size,
			);

			ptr::write_bytes(
				(tls.address() as *const u8 as usize + tdata_size) as *mut u8,
				0,
				tls_size - tdata_size,
			);
		}

		// Associate the TLS memory to the current task.
		let mut current_task_borrowed = core_scheduler().current_task.borrow_mut();
		debug!(
			"Set up TLS for task {} at address {:#X}",
			current_task_borrowed.id,
			align_up!(tls.address(), 32)
		);
		current_task_borrowed.tls = Some(Rc::new(RefCell::new(tls)));
	}

	// Call the actual entry point of the task.
	func(arg);
}

impl TaskFrame for Task {
	fn create_stack_frame(&mut self, func: extern "C" fn(usize), arg: usize) {
		unsafe {
			// Mark the entire stack with 0xCD.
			ptr::write_bytes(self.stacks.stack as *mut u8, 0xCD, DEFAULT_STACK_SIZE);

			// Set a marker for debugging at the very top.
			let mut stack = (self.stacks.stack + DEFAULT_STACK_SIZE - 0x10) as *mut usize;
			*stack = 0xDEAD_BEEFusize;

			// Put the leave_task function on the stack.
			// When the task has finished, it will call this function by returning.
			stack = (stack as usize - mem::size_of::<usize>()) as *mut usize;
			*stack = leave_task as usize;

			// Put the State structure expected by the ASM switch() function on the stack.
			stack = (stack as usize - mem::size_of::<State>()) as *mut usize;

			let state = stack as *mut State;
			ptr::write_bytes(state as *mut u8, 0, mem::size_of::<State>());

			(*state).rip = task_entry as usize;
			(*state).rdi = func as usize;
			(*state).rsi = arg as usize;
			(*state).rflags = 0x1202usize;

			// Set the task's stack pointer entry to the stack we have just crafted.
			self.last_stack_pointer = stack as usize;
		}
	}
}

extern "x86-interrupt" fn timer_handler(_stack_frame: &mut irq::ExceptionStackFrame) {
	core_scheduler().blocked_tasks.lock().handle_waiting_tasks();
	apic::eoi();
	core_scheduler().scheduler();
}

pub fn install_timer_handler() {
	idt::set_gate(apic::TIMER_INTERRUPT_NUMBER, timer_handler as usize, 0);
}
