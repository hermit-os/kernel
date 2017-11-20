// Copyright (c) 2017 Colin Finck, RWTH Aachen University
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

use arch::processor;


extern "C" {
	fn check_scheduling();
	fn check_timers();
	fn shutdown_system();

	static go_down: u32;
}


#[allow(non_camel_case_types)]
type tid_t = u32;

#[repr(C, align(64))]
pub struct task_t {
	/// Task id = position in the task table
	pub id: tid_t,
	/// Task status (INVALID, READY, RUNNING, ...)
	pub status: u32,
	/// last core id on which the task was running
	pub last_core: u32,
	/// copy of the stack pointer before a context switch
	pub last_stack_pointer: usize,
	/// start address of the stack
	pub stack: usize,
	/// interrupt stack for IST1
	//pub ist_addr: usize,
	/// Additional status flags. For instance, to signalize the using of the FPU
	pub flags: u8,
	/// Task priority
	pub prio: u8,
	/// timeout for a blocked task
	pub timeout: u64,
	/// starting time/tick of the task
	pub start_tick: u64,
	/// last TSC, when the task got the CPU
	pub last_tsc: u64,
	/// the userspace heap
	pub heap: *const vma_t,
	/// parent thread
	pub parent: tid_t,
	/// next task in the queue
	pub next: *const task_t,
	/// previous task in the queue
	pub prev: *const task_t,
	/// TLS address
	pub tls_addr: usize,
	/// TLS file size
	pub tls_size: usize,
	/// LwIP error code
	pub lwip_err: i32,
	/// Handler for (POSIX) Signals - TODO!!
	pub signal_handler: usize,
	// FPU state
	pub fpu_state: processor::XSaveArea,
}

#[repr(C)]
pub struct vma_t {
	/// Start address of the memory area
	pub start: usize,
	/// End address of the memory area
	pub end: usize,
	/// Type flags field
	pub flags: u32,
	/// Pointer of next VMA element in the list
	pub next: *const vma_t,
	/// Pointer to previous VMA element in the list
	pub prev: *const vma_t,
}

#[no_mangle]
pub unsafe extern "C" fn check_workqueues_in_irqhandler(irq: i32)
{
	// Increment ticks
	processor::update_ticks();

	check_timers();

	if go_down > 0 {
		shutdown_system();
	}

	if irq < 0 {
		check_scheduling();
	}
}
