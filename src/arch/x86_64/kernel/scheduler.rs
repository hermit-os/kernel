// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//               2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! Architecture dependent interface to initialize a task

use arch::x86_64::kernel::apic;
use arch::x86_64::kernel::idt;
use arch::x86_64::kernel::irq;
use arch::x86_64::kernel::percore::*;
use arch::x86_64::kernel::processor;
use arch::x86_64::mm::paging::{BasePageSize, PageSize};
use config::*;
use core::{mem, ptr};
use environment;
use mm;
use scheduler::task::{Task, TaskFrame};

#[repr(C, packed)]
struct State {
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
	stack_size: usize,
	/// Stack of the task
	stack: usize,
	ist0: usize,
}

impl TaskStacks {
	pub fn new(size: usize) -> Self {
		let stack_size = if size < KERNEL_STACK_SIZE {
			KERNEL_STACK_SIZE
		} else {
			align_up!(size, BasePageSize::SIZE)
		};

		// Allocate an executable stack to possibly support dynamically generated code on the stack (see https://security.stackexchange.com/a/47825).
		let stack = ::mm::allocate(stack_size, false);
		debug!("Allocating stack {:#X}", stack);
		let ist0 = ::mm::allocate(KERNEL_STACK_SIZE, false);
		debug!("Allocating ist0 {:#X}", ist0);

		Self {
			is_boot_stack: false,
			stack_size: stack_size,
			stack: stack,
			ist0: ist0,
		}
	}

	pub fn from_boot_stacks() -> Self {
		let tss = unsafe { &(*PERCORE.tss.get()) };
		let stack = tss.rsp[0] as usize + 0x10 - KERNEL_STACK_SIZE;
		debug!("Using boot stack {:#X}", stack);
		let ist0 = tss.ist[0] as usize + 0x10 - KERNEL_STACK_SIZE;
		debug!("IST0 is located at {:#X}", ist0);

		Self {
			is_boot_stack: true,
			stack_size: KERNEL_STACK_SIZE,
			stack: stack,
			ist0: ist0,
		}
	}

	#[inline]
	pub fn get_stack_size(&self) -> usize {
		self.stack_size
	}

	#[inline]
	pub fn get_stack_address(&self) -> usize {
		self.stack
	}

	#[inline]
	pub fn get_ist0(&self) -> usize {
		self.ist0
	}
}

impl Drop for TaskStacks {
	fn drop(&mut self) {
		if !self.is_boot_stack {
			debug!(
				"Deallocating stack {:#X} and ist0 {:#X}",
				self.stack, self.ist0
			);
			::mm::deallocate(self.stack, self.stack_size);
			::mm::deallocate(self.ist0, KERNEL_STACK_SIZE);
		}
	}
}

pub struct TaskTLS {
	address: usize,
	size: usize,
	fs: usize,
}

impl TaskTLS {
	pub fn new(tls_size: usize) -> Self {
		// determine the size of tdata (tls without tbss)
		let tdata_size: usize = environment::get_tls_filesz();
		// Yes, it does, so we have to allocate TLS memory.
		// Allocate enough space for the given size and one more variable of type usize, which holds the tls_pointer.
		let tls_allocation_size = align_up!(tls_size, 32) + mem::size_of::<usize>();
		// We allocate in BasePageSize granularity, so we don't have to manually impose an
		// additional alignment for TLS variables.
		let memory_size = align_up!(tls_allocation_size, BasePageSize::SIZE);
		let ptr = mm::allocate(memory_size, true);

		// The tls_pointer is the address to the end of the TLS area requested by the task.
		let tls_pointer = ptr + align_up!(tls_size, 32);

		unsafe {
			// Copy over TLS variables with their initial values.
			ptr::copy_nonoverlapping(
				environment::get_tls_start() as *const u8,
				ptr as *mut u8,
				tdata_size,
			);

			ptr::write_bytes(
				(ptr + tdata_size) as *mut u8,
				0,
				align_up!(tls_size, 32) - tdata_size,
			);

			// The x86-64 TLS specification also requires that the tls_pointer can be accessed at fs:0.
			// This allows TLS variable values to be accessed by "mov rax, fs:0" and a later "lea rdx, [rax+VARIABLE_OFFSET]".
			// See "ELF Handling For Thread-Local Storage", version 0.20 by Ulrich Drepper, page 12 for details.
			//
			// fs:0 is where tls_pointer points to and we have reserved space for a usize value above.
			*(tls_pointer as *mut usize) = tls_pointer;
		}

		debug!(
			"Set up TLS at 0x{:x}, tdata_size 0x{:x}, tls_size 0x{:x}",
			tls_pointer, tdata_size, tls_size
		);

		Self {
			address: ptr,
			size: memory_size,
			fs: tls_pointer,
		}
	}

	#[inline]
	pub fn address(&self) -> usize {
		self.address
	}

	#[inline]
	pub fn get_fs(&self) -> usize {
		self.fs
	}
}

impl Drop for TaskTLS {
	fn drop(&mut self) {
		debug!(
			"Deallocate TLS at 0x{:x} (size 0x{:x})",
			self.address, self.size
		);
		mm::deallocate(self.address, self.size);
	}
}

impl Clone for TaskTLS {
	fn clone(&self) -> Self {
		TaskTLS::new(environment::get_tls_memsz())
	}
}

extern "C" fn leave_task() -> ! {
	debug!("Leave task {}", core_scheduler().get_current_task_id());
	core_scheduler().exit(0);
}

#[cfg(test)]
extern "C" fn task_entry(func: extern "C" fn(usize), arg: usize) {}

#[cfg(not(test))]
extern "C" fn task_entry(func: extern "C" fn(usize), arg: usize) {
	debug!(
		"Enter task {} with fs 0x{:x}",
		core_scheduler().get_current_task_id(),
		processor::readfs()
	);

	// Call the actual entry point of the task.
	func(arg);
}

impl TaskFrame for Task {
	fn create_stack_frame(&mut self, func: extern "C" fn(usize), arg: usize) {
		// Check if the task (process or thread) uses Thread-Local-Storage.
		let tls_size = environment::get_tls_memsz();
		self.tls = if tls_size > 0 {
			Some(TaskTLS::new(tls_size))
		} else {
			None
		};

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

			if let Some(tls) = &self.tls {
				(*state).fs = tls.get_fs();
			}
			(*state).rip = task_entry as usize;
			(*state).rdi = func as usize;
			(*state).rsi = arg as usize;

			// per default we disable interrupts
			(*state).rflags = 0x1202usize;

			// Set the task's stack pointer entry to the stack we have just crafted.
			self.last_stack_pointer = stack as usize;
		}
	}
}

extern "x86-interrupt" fn timer_handler(_stack_frame: &mut irq::ExceptionStackFrame) {
	core_scheduler().handle_waiting_tasks();
	apic::eoi();
	core_scheduler().scheduler();
}

pub fn install_timer_handler() {
	idt::set_gate(apic::TIMER_INTERRUPT_NUMBER, timer_handler as usize, 0);
}
