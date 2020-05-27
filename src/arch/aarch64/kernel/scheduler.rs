// Copyright (c) 2018 Stefan Lankes, RWTH Aachen University
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
use core::cell::RefCell;
use core::{mem, ptr};

use crate::arch::aarch64::kernel::percore::*;
use crate::arch::aarch64::kernel::processor;
use crate::scheduler::task::{Task, TaskFrame, TaskTLS};

include!(concat!(env!("CARGO_TARGET_DIR"), "/config.rs"));

extern "C" {
	static tls_start: u8;
	static tls_end: u8;
}

pub struct TaskStacks {
	is_boot_stack: bool,
	stack: usize,
}

impl TaskStacks {
	pub fn new() -> Self {
		// TODO: Allocate
		Self {
			is_boot_stack: false,
			stack: 0,
		}
	}

	pub fn from_boot_stacks() -> Self {
		// TODO: Get boot stacks
		Self {
			is_boot_stack: true,
			stack: 0,
		}
	}
}

impl Drop for TaskStacks {
	fn drop(&mut self) {
		if !self.is_boot_stack {
			// TODO: Deallocate
		}
	}
}

extern "C" fn leave_task() -> ! {
	core_scheduler().exit(0)
}

extern "C" fn task_entry(func: extern "C" fn(usize), arg: usize) {
	// Check if the task (process or thread) uses Thread-Local-Storage.
	let tls_size = unsafe { &tls_end as *const u8 as usize - &tls_start as *const u8 as usize };
	if tls_size > 0 {
		// Yes, it does, so we have to allocate TLS memory.
		// Allocate enough space for the given size and one more variable of type usize, which holds the tls_pointer.
		let tls_allocation_size = tls_size + mem::size_of::<usize>();
		let tls = TaskTLS::new(tls_allocation_size);

		// The tls_pointer is the address to the end of the TLS area requested by the task.
		let tls_pointer = tls.address() + tls_size;

		// TODO: Implement AArch64 TLS

		// Associate the TLS memory to the current task.
		let mut current_task_borrowed = core_scheduler().current_task.borrow_mut();
		debug!(
			"Set up TLS for task {} at address {:#X}",
			current_task_borrowed.id,
			tls.address()
		);
		current_task_borrowed.tls = Some(Rc::new(RefCell::new(tls)));
	}

	// Call the actual entry point of the task.
	func(arg);
}

impl TaskFrame for Task {
	fn create_stack_frame(&mut self, func: extern "C" fn(usize), arg: usize) {
		// TODO: Implement AArch64 stack frame
	}
}
