// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//               2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! Architecture dependent interface to initialize a task

use alloc::alloc::{alloc, dealloc, Layout};
use core::{mem, ptr};

use crate::arch::x86_64::kernel::apic;
use crate::arch::x86_64::kernel::idt;
use crate::arch::x86_64::kernel::irq;
use crate::arch::x86_64::kernel::percore::*;
use crate::arch::x86_64::mm::paging::{BasePageSize, PageSize, PageTableEntryFlags};
use crate::config::*;
use crate::environment;
use crate::scheduler::task::{Task, TaskFrame};

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

pub struct BootStack {
	/// stack for kernel tasks
	stack: usize,
	/// stack to handle interrupts
	ist0: usize,
}

pub struct CommonStack {
	/// start address of allocated virtual memory region
	virt_addr: usize,
	/// start address of allocated virtual memory region
	phys_addr: usize,
	/// total size of all stacks
	total_size: usize,
}

pub enum TaskStacks {
	Boot(BootStack),
	Common(CommonStack),
}

impl TaskStacks {
	pub fn new(size: usize) -> TaskStacks {
		let user_stack_size = if size < KERNEL_STACK_SIZE {
			KERNEL_STACK_SIZE
		} else {
			align_up!(size, BasePageSize::SIZE)
		};
		let total_size = user_stack_size + DEFAULT_STACK_SIZE + KERNEL_STACK_SIZE;
		let virt_addr = crate::arch::mm::virtualmem::allocate(total_size + 4 * BasePageSize::SIZE)
			.expect("Failed to allocate Virtual Memory for TaskStacks");
		let phys_addr = crate::arch::mm::physicalmem::allocate(total_size)
			.expect("Failed to allocate Physical Memory for TaskStacks");

		debug!(
			"Create stacks at {:#X} with a size of {} KB",
			virt_addr,
			total_size >> 10
		);

		let mut flags = PageTableEntryFlags::empty();
		flags.normal().writable().execute_disable();

		// map IST0 into the address space
		crate::arch::mm::paging::map::<BasePageSize>(
			virt_addr + BasePageSize::SIZE,
			phys_addr,
			KERNEL_STACK_SIZE / BasePageSize::SIZE,
			flags,
		);

		// map kernel stack into the address space
		crate::arch::mm::paging::map::<BasePageSize>(
			virt_addr + KERNEL_STACK_SIZE + 2 * BasePageSize::SIZE,
			phys_addr + KERNEL_STACK_SIZE,
			DEFAULT_STACK_SIZE / BasePageSize::SIZE,
			flags,
		);

		// map user stack into the address space
		crate::arch::mm::paging::map::<BasePageSize>(
			virt_addr + KERNEL_STACK_SIZE + DEFAULT_STACK_SIZE + 3 * BasePageSize::SIZE,
			phys_addr + KERNEL_STACK_SIZE + DEFAULT_STACK_SIZE,
			user_stack_size / BasePageSize::SIZE,
			flags,
		);

		// clear user stack
		unsafe {
			ptr::write_bytes(
				(virt_addr + KERNEL_STACK_SIZE + DEFAULT_STACK_SIZE + 3 * BasePageSize::SIZE)
					as *mut u8,
				0xAC,
				user_stack_size,
			);
		}

		TaskStacks::Common(CommonStack {
			virt_addr,
			phys_addr,
			total_size,
		})
	}

	pub fn from_boot_stacks() -> TaskStacks {
		let tss = unsafe { &(*PERCORE.tss.get()) };
		let stack = tss.rsp[0] as usize + 0x10 - KERNEL_STACK_SIZE;
		debug!("Using boot stack {:#X}", stack);
		let ist0 = tss.ist[0] as usize + 0x10 - KERNEL_STACK_SIZE;
		debug!("IST0 is located at {:#X}", ist0);

		TaskStacks::Boot(BootStack { stack, ist0 })
	}

	pub fn get_user_stack_size(&self) -> usize {
		match self {
			TaskStacks::Boot(_) => 0,
			TaskStacks::Common(stacks) => {
				stacks.total_size - DEFAULT_STACK_SIZE - KERNEL_STACK_SIZE
			}
		}
	}

	pub fn get_user_stack(&self) -> usize {
		match self {
			TaskStacks::Boot(_) => 0,
			TaskStacks::Common(stacks) => {
				stacks.virt_addr + KERNEL_STACK_SIZE + DEFAULT_STACK_SIZE + 3 * BasePageSize::SIZE
			}
		}
	}

	pub fn get_kernel_stack(&self) -> usize {
		match self {
			TaskStacks::Boot(stacks) => stacks.stack,
			TaskStacks::Common(stacks) => {
				stacks.virt_addr + KERNEL_STACK_SIZE + 2 * BasePageSize::SIZE
			}
		}
	}

	pub fn get_kernel_stack_size(&self) -> usize {
		match self {
			TaskStacks::Boot(_) => KERNEL_STACK_SIZE,
			TaskStacks::Common(_) => DEFAULT_STACK_SIZE,
		}
	}

	pub fn get_interupt_stack(&self) -> usize {
		match self {
			TaskStacks::Boot(stacks) => stacks.ist0,
			TaskStacks::Common(stacks) => stacks.virt_addr + BasePageSize::SIZE,
		}
	}

	pub fn get_interupt_stack_size(&self) -> usize {
		KERNEL_STACK_SIZE
	}
}

impl Drop for TaskStacks {
	fn drop(&mut self) {
		// we should never deallocate a boot stack
		match self {
			TaskStacks::Boot(_) => {}
			TaskStacks::Common(stacks) => {
				debug!(
					"Deallocating stacks at {:#X} with a size of {} KB",
					stacks.virt_addr,
					stacks.total_size >> 10,
				);

				crate::arch::mm::paging::unmap::<BasePageSize>(
					stacks.virt_addr,
					stacks.total_size / BasePageSize::SIZE + 4,
				);
				crate::arch::mm::virtualmem::deallocate(
					stacks.virt_addr,
					stacks.total_size + 4 * BasePageSize::SIZE,
				);
				crate::arch::mm::physicalmem::deallocate(stacks.phys_addr, stacks.total_size);
			}
		}
	}
}

impl Clone for TaskStacks {
	fn clone(&self) -> TaskStacks {
		match self {
			TaskStacks::Boot(_) => TaskStacks::new(0),
			TaskStacks::Common(stacks) => {
				TaskStacks::new(stacks.total_size - DEFAULT_STACK_SIZE - KERNEL_STACK_SIZE)
			}
		}
	}
}

pub struct TaskTLS {
	address: usize,
	fs: usize,
	layout: Layout,
}

impl TaskTLS {
	pub fn new(tls_size: usize) -> Self {
		// determine the size of tdata (tls without tbss)
		let tdata_size: usize = environment::get_tls_filesz();
		// Yes, it does, so we have to allocate TLS memory.
		// Allocate enough space for the given size and one more variable of type usize, which holds the tls_pointer.
		let tls_allocation_size = align_up!(tls_size, 32) + mem::size_of::<usize>();
		// We allocate in 128 byte granularity (= cache line size) to avoid false sharing
		let memory_size = align_up!(tls_allocation_size, 128);
		let layout =
			Layout::from_size_align(memory_size, 128).expect("TLS has an invalid size / alignment");
		let ptr = unsafe { alloc(layout) } as usize;

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
			fs: tls_pointer,
			layout: layout,
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
			"Deallocate TLS at 0x{:x} (layout {:?})",
			self.address, self.layout,
		);

		unsafe {
			dealloc(self.address as *mut u8, self.layout);
		}
	}
}

impl Clone for TaskTLS {
	fn clone(&self) -> Self {
		TaskTLS::new(environment::get_tls_memsz())
	}
}

#[cfg(test)]
extern "C" fn task_entry(func: extern "C" fn(usize), arg: usize) {}

#[cfg(not(test))]
#[inline(never)]
#[naked]
extern "C" fn task_entry(func: extern "C" fn(usize), arg: usize) -> ! {
	// Call the actual entry point of the task.
	switch_to_user!();
	func(arg);
	switch_to_kernel!();

	// Exit task
	core_scheduler().exit(0)
}

impl TaskFrame for Task {
	fn create_stack_frame(&mut self, func: extern "C" fn(usize), arg: usize) {
		// Check if the task (process or thread) uses Thread-Local-Storage.
		let tls_size = environment::get_tls_memsz();
		// check is TLS is already allocated
		if self.tls.is_none() && tls_size > 0 {
			self.tls = Some(TaskTLS::new(tls_size))
		}

		unsafe {
			// Set a marker for debugging at the very top.
			let mut stack = (self.stacks.get_kernel_stack() + self.stacks.get_kernel_stack_size()
				- 0x10) as *mut usize;
			*stack = 0xDEAD_BEEFusize;

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
			self.user_stack_pointer =
				self.stacks.get_user_stack() + self.stacks.get_user_stack_size() - 0x10;
		}
	}
}

extern "x86-interrupt" fn timer_handler(_stack_frame: &mut irq::ExceptionStackFrame) {
	increment_irq_counter(apic::TIMER_INTERRUPT_NUMBER.into());
	core_scheduler().handle_waiting_tasks();
	apic::eoi();
	core_scheduler().scheduler();
}

pub fn install_timer_handler() {
	idt::set_gate(apic::TIMER_INTERRUPT_NUMBER, timer_handler as usize, 0);
	irq::add_irq_name((apic::TIMER_INTERRUPT_NUMBER - 32).into(), "Timer");
}
