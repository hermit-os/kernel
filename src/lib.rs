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

#[cfg(test)]
#[macro_use]
extern crate std;

// EXTERNAL CRATES
#[macro_use]
extern crate alloc;
#[macro_use]
extern crate bitflags;
#[cfg(target_arch = "x86_64")]
extern crate hermit_multiboot;
extern crate spin;
#[cfg(target_arch = "x86_64")]
extern crate x86;
#[macro_use]
extern crate log;
extern crate smoltcp;

#[macro_use]
mod macros;

#[macro_use]
mod logging;

mod arch;
mod collections;
mod config;
mod console;
mod drivers;
mod environment;
mod errno;
#[cfg(not(test))]
mod kernel_message_buffer;
mod mm;
mod runtime_glue;
mod scheduler;
mod synch;
mod syscalls;

pub use arch::*;
pub use config::*;
pub use syscalls::*;

use alloc::alloc::Layout;
use arch::percore::*;
use core::alloc::GlobalAlloc;
use core::ptr;
use mm::allocator::LockedHeap;

#[cfg(not(test))]
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn sys_malloc(size: usize, align: usize) -> *mut u8 {
	let layout: Layout = Layout::from_size_align(size, align).unwrap();
	let ptr;

	unsafe {
		ptr = ALLOCATOR.alloc(layout);
	}

	trace!(
		"sys_malloc: allocate memory at 0x{:x} (size 0x{:x}, align 0x{:x})",
		ptr as usize,
		size,
		align
	);

	ptr
}

#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn sys_realloc(ptr: *mut u8, size: usize, align: usize, new_size: usize) -> *mut u8 {
	let layout: Layout = Layout::from_size_align(size, align).unwrap();
	let new_ptr;

	unsafe {
		new_ptr = ALLOCATOR.realloc(ptr, layout, new_size);
	}

	trace!(
		"sys_realloc: resize memory at 0x{:x}, new address 0x{:x}",
		ptr as usize,
		new_ptr as usize
	);

	new_ptr
}

#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn sys_free(ptr: *mut u8, size: usize, align: usize) {
	let layout: Layout = Layout::from_size_align(size, align).unwrap();

	trace!(
		"sys_free: deallocate memory at 0x{:x} (size 0x{:x})",
		ptr as usize,
		size
	);

	unsafe {
		ALLOCATOR.dealloc(ptr, layout);
	}
}

#[cfg(not(test))]
extern "C" {
	static mut __bss_start: u8;
	static mut hbss_start: u8;
	static kernel_start: u8;
}

#[cfg(not(test))]
unsafe fn sections_init() {
	// Initialize .kbss sections for the kernel.
	ptr::write_bytes(
		&mut hbss_start as *mut u8,
		0,
		&__bss_start as *const u8 as usize - &hbss_start as *const u8 as usize,
	);
}

#[cfg(not(test))]
extern "C" fn initd(_arg: usize) {
	extern "C" {
		fn runtime_entry(argc: i32, argv: *mut *mut u8, env: *mut *mut u8) -> !;
	}

	if environment::is_uhyve() {
		// Initialize the uhyve-net interface using the IP and gateway addresses specified in hcip, hcmask, hcgateway.
		info!("HermitCore is running on uhyve!");
		let _ = drivers::net::uhyve::init();
	} else if !environment::is_single_kernel() {
		// Initialize the mmnif interface using static IPs in the range 192.168.28.x.
		info!("HermitCore is running side-by-side to Linux!");
	} else {
		let _ = drivers::net::rtl8139::init();
	}

	syscalls::init();

	// Get the application arguments and environment variables.
	let (argc, argv, environ) = syscalls::get_application_parameters();

	// give the IP thread time to initialize the network interface
	core_scheduler().scheduler();

	unsafe {
		// Initialize .bss sections for the application.
		ptr::write_bytes(
			&mut __bss_start as *mut u8,
			0,
			&kernel_start as *const u8 as usize + environment::get_image_size()
				- &__bss_start as *const u8 as usize,
		);

		// And finally start the application.
		runtime_entry(argc, argv, environ);
	}
}

/// Entry Point of HermitCore for the Boot Processor
/// (called from entry.asm)
#[cfg(not(test))]
pub fn boot_processor_main() -> ! {
	// Initialize the kernel and hardware.
	unsafe {
		sections_init();
	}
	arch::message_output_init();
	logging::init();

	info!(
		"Welcome to HermitCore-rs {} ({})",
		env!("CARGO_PKG_VERSION"),
		COMMIT_HASH
	);
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
		scheduler::task::NORMAL_PRIO,
		Some(arch::mm::virtualmem::task_heap_start()),
	);

	// Run the scheduler loop.
	loop {
		core_scheduler.scheduler();
	}
}

/// Entry Point of HermitCore for an Application Processor
/// (called from entry.asm)
#[cfg(not(test))]
pub fn application_processor_main() -> ! {
	arch::application_processor_init();
	scheduler::add_current_core();
	let core_scheduler = core_scheduler();

	// Run the scheduler loop.
	loop {
		core_scheduler.scheduler();
	}
}
