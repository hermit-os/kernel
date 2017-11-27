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

#[macro_use]
extern crate lazy_static;

extern crate multiboot;
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
mod dummies;
mod mm;
mod runtime_glue;
mod synch;
mod tasks;
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
pub use tasks::*;
pub use timer::*;

use consts::*;
use core::ptr;
use mm::allocator;

#[global_allocator]
static ALLOCATOR: allocator::HermitAllocator = allocator::HermitAllocator;


extern "C" {
	static __bss_start: u8;
	static mut hbss_start: u8;
	static percore_start: u8;
	static percore_end0: u8;

	fn koutput_init() -> i32;
	fn multitasking_init() -> i32;
	fn hermit_main() -> i32;
}

// FUNCTIONS
unsafe fn sections_init() {
	// Initialize .kbss sections
	ptr::write_bytes(
		&mut hbss_start as *mut u8,
		0,
		&__bss_start as *const u8 as usize - &hbss_start as *const u8 as usize
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

/// Entry Point of HermitCore for the Boot Processor
/// (called from entry.asm)
#[no_mangle]
pub unsafe extern "C" fn boot_processor_main() {
	sections_init();
	koutput_init();

	info!("Welcome to HermitCore {}!", env!("CARGO_PKG_VERSION"));
	arch::boot_processor_init();

	//multitasking_init();
	//hermit_main();
}

/// Entry Point of HermitCore for an Application Processor
// (called from entry.asm)
#[no_mangle]
pub unsafe extern "C" fn application_processor_main() {
	arch::application_processor_init();
}
