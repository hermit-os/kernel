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

/*
 * First version is derived and adapted for HermitCore from
 * Philipp Oppermann's excellent series of blog posts (http://blog.phil-opp.com/)
 * and Eric Kidd's toy OS (https://github.com/emk/toyos-rs).
 */

#![feature(abi_x86_interrupt)]
#![feature(alloc)]
#![feature(allocator_api)]
#![feature(asm)]
#![feature(attr_literals)]
#![feature(const_fn)]
#![feature(const_atomic_bool_new)]
#![feature(const_atomic_usize_new)]
#![feature(const_unsafe_cell_new)]
#![feature(global_allocator)]
#![feature(hint_core_should_pause)]
#![feature(iterator_step_by)]
#![feature(lang_items)]
#![feature(linkage)]
#![feature(repr_align)]
#![feature(specialization)]
#![no_std]

// EXTERNAL CRATES
extern crate alloc;

#[macro_use]
extern crate bitflags;

extern crate hermit_multiboot;

#[macro_use]
extern crate lazy_static;

extern crate raw_cpuid;
extern crate spin;
extern crate x86;

// MODULES
#[macro_use]
mod macros;

#[macro_use]
mod logging;

mod arch;
mod collections;
mod console;
mod consts;
mod drivers;
mod dummies;
mod errno;
mod mm;
mod runtime_glue;
mod scheduler;
mod synch;
mod syscalls;
mod timer;

// IMPORTS
#[cfg(target_arch="x86_64")]
mod arch_specific {
	pub use arch::gdt::*;
	pub use arch::irq::*;
	pub use arch::mm::paging::*;
	pub use arch::processor::*;
}

pub use arch_specific::*;
pub use dummies::*;
pub use syscalls::*;
pub use timer::*;

use arch::percore::*;
use consts::*;
use core::ptr;
use mm::allocator;

#[global_allocator]
static ALLOCATOR: allocator::HermitAllocator = allocator::HermitAllocator;


extern "C" {
	static mut __bss_start: u8;
	static mut hbss_start: u8;
	static mut libc_sd: i32;
	static kernel_end: u8;
	static percore_start: u8;
	static percore_end0: u8;

	fn libc_start(argc: i32, argv: *mut *mut u8, env: *mut *mut u8);
}

// FUNCTIONS
unsafe fn sections_init() {
	// Initialize .kbss sections for the kernel.
	ptr::write_bytes(
		&mut hbss_start as *mut u8,
		0,
		&__bss_start as *const u8 as usize - &hbss_start as *const u8 as usize
	);

	// Initialize .bss sections for the user program.
	ptr::write_bytes(
		&mut __bss_start as *mut u8,
		0,
		&kernel_end as *const u8 as usize - &__bss_start as *const u8 as usize
	);

	// Initialize .percore sections
	// Copy the section for the first CPU to all others.
	let size = &percore_end0 as *const u8 as usize - &percore_start as *const u8 as usize;
	for i in 1..MAX_CORES {
		ptr::copy_nonoverlapping(
			&percore_start as *const u8,
			(&percore_start as *const u8 as usize + i*size) as *mut u8,
			size
		);
	}
}

extern "C" fn initd(_arg: usize) {
	// TODO: Setup Heap
	// TODO: Setup Networking
	// TODO: argc, argv, environ

	let argc = 0;
	let argv = 0 as *mut *mut u8;
	let environ = 0 as *mut *mut u8;
	unsafe { libc_start(argc, argv, environ); }
}

/// Entry Point of HermitCore for the Boot Processor
/// (called from entry.asm)
#[no_mangle]
pub unsafe extern "C" fn boot_processor_main() {
	// Initialize the kernel and hardware.
	sections_init();
	arch::message_output_init();

	info!("Welcome to HermitCore {}!", env!("CARGO_PKG_VERSION"));
	arch::boot_processor_init();
	scheduler::init();
	scheduler::add_current_core();
	arch::boot_application_processors();

	// Start the initd task.
	let core_scheduler = scheduler::get_scheduler(core_id());
	core_scheduler.spawn(
		initd,
		0,
		scheduler::task::REALTIME_PRIO,
		Some(arch::mm::virtualmem::task_heap_start())
	);

	// Run the scheduler loop for the boot processor.
	loop {
		core_scheduler.reschedule();
		if scheduler::number_of_tasks() == 0 {
			arch::processor::shutdown();
		}
		arch::processor::halt();
	}
}

/// Entry Point of HermitCore for an Application Processor
/// (called from entry.asm)
#[no_mangle]
pub unsafe extern "C" fn application_processor_main() {
	arch::application_processor_init();
	scheduler::add_current_core();
	let core_scheduler = scheduler::get_scheduler(core_id());

	loop {
		core_scheduler.reschedule();
		arch::processor::halt();
	}
}
