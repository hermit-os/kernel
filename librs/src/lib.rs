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
#![feature(const_atomic_isize_new)]
#![feature(const_atomic_usize_new)]
#![feature(const_unsafe_cell_new)]
#![feature(integer_atomics)]
#![feature(iterator_step_by)]
#![feature(lang_items)]
#![feature(linkage)]
#![feature(panic_implementation)]
#![feature(panic_info_message)]
#![feature(specialization)]
#![allow(unused_macros)]
#![no_std]

include!(concat!(env!("CARGO_TARGET_DIR"), "/config.rs"));

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
mod environment;
mod errno;
mod kernel_message_buffer;
mod mm;
mod runtime_glue;
mod scheduler;
mod synch;
mod syscalls;

// IMPORTS
pub use arch::*;
pub use syscalls::*;

use arch::percore::*;
use processor::get_frequency;
use core::ptr;
use mm::allocator;

#[global_allocator]
static ALLOCATOR: &'static allocator::HermitAllocator = &allocator::HermitAllocator;


extern "C" {
	static mut __bss_start: u8;
	static mut hbss_start: u8;
	static kernel_start: u8;
	static image_size: u64;

	fn libc_start(argc: i32, argv: *mut *mut u8, env: *mut *mut u8);
	fn init_lwip();
	fn init_rtl8139_netif(freq: u32) -> i32;
	fn init_uhyve_netif() -> i32;
}

// FUNCTIONS
unsafe fn sections_init() {
	// Initialize .kbss sections for the kernel.
	ptr::write_bytes(
		&mut hbss_start as *mut u8,
		0,
		&__bss_start as *const u8 as usize - &hbss_start as *const u8 as usize
	);
}

extern "C" fn initd(_arg: usize) {
	// initialize LwIP library
	unsafe { init_lwip(); }

	// Initialize the specific network interface.
	let mut err = 0;

	if environment::is_uhyve() {
		// Initialize the uhyve-net interface using the IP and gateway addresses specified in hcip, hcmask, hcgateway.
		info!("HermitCore is running on uhyve!");
		unsafe { init_uhyve_netif(); }
	} else if !environment::is_single_kernel() {
		// Initialize the mmnif interface using static IPs in the range 192.168.28.x.
		info!("HermitCore is running side-by-side to Linux!");
		//unsafe { init_mmnif_netif(); }
	} else {
		// Initialize the RTL8139 interface using DHCP.
		err = unsafe { init_rtl8139_netif(get_frequency() as u32) };
	}

	// Check if a network interface has been initialized.
	if err == 0 {
		info!("Successfully initialized a network interface!");
		syscalls::enable_networking();
	} else {
		warn!("Could not initialize a network interface (error code {})", err);
		warn!("Starting HermitCore without network support");
	}

	// Get the application arguments and environment variables.
	let (argc, argv, environ) = syscalls::get_application_parameters();

	unsafe {
		// Initialize .bss sections for the application.
		ptr::write_bytes(
			&mut __bss_start as *mut u8,
			0,
			&kernel_start as *const u8 as usize + image_size as usize - &__bss_start as *const u8 as usize
		);

		// And finally start the application.
		libc_start(argc, argv, environ);
	}
}

/// Entry Point of HermitCore for the Boot Processor
/// (called from entry.asm)
#[no_mangle]
pub unsafe extern "C" fn boot_processor_main() {
	// Initialize the kernel and hardware.
	sections_init();
	arch::message_output_init();

	info!("Welcome to HermitCore-rs {} ({})", env!("CARGO_PKG_VERSION"), COMMIT_HASH);
	arch::boot_processor_init();
	scheduler::init();
	scheduler::add_current_core();

	if environment::is_single_kernel() && !environment::is_uhyve() {
		arch::boot_application_processors();
	}

	// Start the initd task.
	let core_scheduler = core_scheduler();
	core_scheduler.spawn(
		initd,
		0,
		scheduler::task::HIGH_PRIO,
		Some(arch::mm::virtualmem::task_heap_start())
	);

	// Run the scheduler loop.
	loop {
		core_scheduler.scheduler();
	}
}

/// Entry Point of HermitCore for an Application Processor
/// (called from entry.asm)
#[no_mangle]
pub unsafe extern "C" fn application_processor_main() {
	arch::application_processor_init();
	scheduler::add_current_core();
	let core_scheduler = core_scheduler();

	// Run the scheduler loop.
	loop {
		core_scheduler.scheduler();
	}
}
