// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//                    Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

/*
 * First version is derived and adapted for HermitCore from
 * Philipp Oppermann's excellent series of blog posts (http://blog.phil-opp.com/)
 * and Eric Kidd's toy OS (https://github.com/emk/toyos-rs).
 */

#![feature(abi_x86_interrupt)]
#![feature(allocator_api)]
#![feature(asm)]
#![feature(const_fn)]
#![feature(lang_items)]
#![feature(linkage)]
#![feature(panic_info_message)]
#![feature(specialization)]
#![feature(naked_functions)]
#![feature(core_intrinsics)]
#![feature(const_vec_new)]
#![allow(unused_macros)]
#![no_std]

include!(concat!(env!("CARGO_TARGET_DIR"), "/config.rs"));

// EXTERNAL CRATES
extern crate alloc;

#[macro_use]
extern crate bitflags;

#[cfg(target_arch = "x86_64")]
extern crate hermit_multiboot;

extern crate spin;

#[cfg(target_arch = "x86_64")]
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
use mm::allocator;
use core::ptr;
use core::alloc::GlobalAlloc;
use alloc::alloc::Layout;

#[global_allocator]
static ALLOCATOR: &'static allocator::HermitAllocator = &allocator::HermitAllocator;

#[no_mangle]
pub extern "C" fn sys_malloc(size: usize, align: usize) -> *mut u8 {
    let layout: Layout = Layout::from_size_align(size, align).unwrap();

    unsafe {
        ALLOCATOR.alloc(layout)
    }
}

#[no_mangle]
pub extern "C" fn sys_realloc(ptr: *mut u8, size: usize, align: usize, new_size: usize) -> *mut u8 {
    let layout: Layout = Layout::from_size_align(size, align).unwrap();

    unsafe {
        ALLOCATOR.realloc(ptr, layout, new_size)
    }
}

#[no_mangle]
pub extern "C" fn sys_free(ptr: *mut u8, size: usize, align: usize) {
    let layout: Layout = Layout::from_size_align(size, align).unwrap();

    unsafe {
        ALLOCATOR.dealloc(ptr, layout);
    }
}

extern "C" {
	static mut __bss_start: u8;
	static mut hbss_start: u8;
	static kernel_start: u8;

	fn libc_start(argc: i32, argv: *mut *mut u8, env: *mut *mut u8) -> !;
	fn init_lwip();
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
		err = arch::network_adapter_init();
	}

	// Check if a network interface has been initialized.
	if err == 0 {
		info!("Successfully initialized a network interface!");
	} else {
		warn!("Could not initialize a network interface (error code {})", err);
		warn!("Starting HermitCore without network support");
	}

	syscalls::init();

	// Get the application arguments and environment variables.
	let (argc, argv, environ) = syscalls::get_application_parameters();

	unsafe {
		// Initialize .bss sections for the application.
		ptr::write_bytes(
			&mut __bss_start as *mut u8,
			0,
			&kernel_start as *const u8 as usize + environment::get_image_size() - &__bss_start as *const u8 as usize
		);

		// And finally start the application.
		libc_start(argc, argv, environ);
	}
}

/// Entry Point of HermitCore for the Boot Processor
/// (called from entry.asm)
pub fn boot_processor_main() -> ! {
	// Initialize the kernel and hardware.
	unsafe { sections_init(); }
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
pub fn application_processor_main() -> ! {
	arch::application_processor_init();
	scheduler::add_current_core();
	let core_scheduler = core_scheduler();

	// Run the scheduler loop.
	loop {
		core_scheduler.scheduler();
	}
}
