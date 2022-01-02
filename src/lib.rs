/*
 * First version is derived and adapted for HermitCore from
 * Philipp Oppermann's excellent series of blog posts (http://blog.phil-opp.com/)
 * and Eric Kidd's toy OS (https://github.com/emk/toyos-rs).
 */

#![warn(rust_2018_idioms)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![allow(clippy::missing_safety_doc)]
#![allow(incomplete_features)]
#![feature(abi_x86_interrupt)]
#![feature(allocator_api)]
#![feature(asm)]
#![feature(asm_const)]
#![feature(asm_sym)]
#![feature(const_btree_new)]
#![feature(const_fn_trait_bound)]
#![feature(const_mut_refs)]
#![feature(const_ptr_offset_from)]
#![feature(lang_items)]
#![feature(linkage)]
#![feature(linked_list_cursors)]
#![feature(naked_functions)]
#![feature(new_uninit)]
#![feature(panic_info_message)]
#![feature(specialization)]
#![feature(nonnull_slice_from_raw_parts)]
#![feature(core_intrinsics)]
#![feature(alloc_error_handler)]
#![feature(vec_into_raw_parts)]
#![feature(drain_filter)]
#![feature(llvm_asm)]
#![feature(global_asm)]
#![no_std]
#![cfg_attr(
	any(target_os = "none", target_os = "hermit"),
	feature(custom_test_frameworks)
)]
#![cfg_attr(
	any(target_os = "none", target_os = "hermit"),
	cfg_attr(test, test_runner(crate::test_runner))
)]
#![cfg_attr(
	any(target_os = "none", target_os = "hermit"),
	cfg_attr(test, reexport_test_harness_main = "test_main")
)]
#![cfg_attr(any(target_os = "none", target_os = "hermit"), cfg_attr(test, no_main))]

// EXTERNAL CRATES
#[macro_use]
extern crate alloc;
#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate log;
#[macro_use]
extern crate num_derive;
#[cfg(not(any(target_os = "none", target_os = "hermit")))]
#[macro_use]
extern crate std;
#[cfg(target_arch = "aarch64")]
extern crate aarch64;
#[cfg(target_arch = "x86_64")]
extern crate x86;

use alloc::alloc::Layout;
use core::alloc::GlobalAlloc;
#[cfg(feature = "smp")]
use core::hint::spin_loop;
#[cfg(feature = "smp")]
use core::sync::atomic::{AtomicU32, Ordering};

use arch::percore::*;
use mm::allocator::LockedHeap;

#[cfg(target_arch = "aarch64")]
use qemu_exit::QEMUExit;

pub use crate::arch::*;
pub use crate::config::*;
pub use crate::syscalls::*;

#[macro_use]
mod macros;

#[macro_use]
mod logging;

mod arch;
mod collections;
mod config;
mod console;
mod drivers;
pub mod environment;
mod errno;
mod ffi;
mod kernel_message_buffer;
mod mm;
#[cfg(any(target_os = "none", target_os = "hermit"))]
mod runtime_glue;
mod scheduler;
mod synch;
mod syscalls;
mod util;

#[doc(hidden)]
fn _print(args: ::core::fmt::Arguments<'_>) {
	use core::fmt::Write;
	crate::console::CONSOLE.lock().write_fmt(args).unwrap();
}

#[cfg(test)]
#[cfg(any(target_os = "none", target_os = "hermit"))]
#[no_mangle]
extern "C" fn runtime_entry(_argc: i32, _argv: *const *const u8, _env: *const *const u8) -> ! {
	println!("Executing hermit unittests. Any arguments are dropped");
	test_main();
	sys_exit(0);
}

//https://github.com/rust-lang/rust/issues/50297#issuecomment-524180479
#[cfg(test)]
pub fn test_runner(tests: &[&dyn Fn()]) {
	println!("Running {} tests", tests.len());
	for test in tests {
		test();
	}
	sys_exit(0);
}

#[cfg(any(target_os = "none", target_os = "hermit"))]
#[test_case]
fn trivial_test() {
	println!("Test test test");
	panic!("Test called");
}

#[cfg(any(target_os = "none", target_os = "hermit"))]
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Interface to allocate memory from system heap
///
/// # Errors
/// Returning a null pointer indicates that either memory is exhausted or
/// `size` and `align` do not meet this allocator's size or alignment constraints.
///
#[cfg(any(target_os = "none", target_os = "hermit"))]
pub extern "C" fn __sys_malloc(size: usize, align: usize) -> *mut u8 {
	let layout_res = Layout::from_size_align(size, align);
	if layout_res.is_err() || size == 0 {
		warn!(
			"__sys_malloc called with size {:#x}, align {:#x} is an invalid layout!",
			size, align
		);
		return core::ptr::null::<*mut u8>() as *mut u8;
	}
	let layout = layout_res.unwrap();
	let ptr = unsafe { ALLOCATOR.alloc(layout) };

	trace!(
		"__sys_malloc: allocate memory at {:#x} (size {:#x}, align {:#x})",
		ptr as usize,
		size,
		align
	);

	ptr
}

/// Shrink or grow a block of memory to the given `new_size`. The block is described by the given
/// ptr pointer and layout. If this returns a non-null pointer, then ownership of the memory block
/// referenced by ptr has been transferred to this allocator. The memory may or may not have been
/// deallocated, and should be considered unusable (unless of course it was transferred back to the
/// caller again via the return value of this method). The new memory block is allocated with
/// layout, but with the size updated to new_size.
/// If this method returns null, then ownership of the memory block has not been transferred to this
/// allocator, and the contents of the memory block are unaltered.
///
/// # Safety
/// This function is unsafe because undefined behavior can result if the caller does not ensure all
/// of the following:
/// - `ptr` must be currently allocated via this allocator,
/// - `size` and `align` must be the same layout that was used to allocate that block of memory.
/// ToDO: verify if the same values for size and align always lead to the same layout
///
/// # Errors
/// Returns null if the new layout does not meet the size and alignment constraints of the
/// allocator, or if reallocation otherwise fails.
#[cfg(any(target_os = "none", target_os = "hermit"))]
pub extern "C" fn __sys_realloc(
	ptr: *mut u8,
	size: usize,
	align: usize,
	new_size: usize,
) -> *mut u8 {
	unsafe {
		let layout_res = Layout::from_size_align(size, align);
		if layout_res.is_err() || size == 0 || new_size == 0 {
			warn!(
			"__sys_realloc called with ptr {:#x}, size {:#x}, align {:#x}, new_size {:#x} is an invalid layout!",
			ptr as usize, size, align, new_size
		);
			return core::ptr::null::<*mut u8>() as *mut u8;
		}
		let layout = layout_res.unwrap();
		let new_ptr = ALLOCATOR.realloc(ptr, layout, new_size);

		if new_ptr.is_null() {
			debug!(
			"__sys_realloc failed to resize ptr {:#x} with size {:#x}, align {:#x}, new_size {:#x} !",
			ptr as usize, size, align, new_size
		);
		} else {
			trace!(
				"__sys_realloc: resized memory at {:#x}, new address {:#x}",
				ptr as usize,
				new_ptr as usize
			);
		}
		new_ptr
	}
}

/// Interface to deallocate a memory region from the system heap
///
/// # Safety
/// This function is unsafe because undefined behavior can result if the caller does not ensure all of the following:
/// - ptr must denote a block of memory currently allocated via this allocator,
/// - `size` and `align` must be the same values that were used to allocate that block of memory
/// ToDO: verify if the same values for size and align always lead to the same layout
///
/// # Errors
/// May panic if debug assertions are enabled and invalid parameters `size` or `align` where passed.
#[cfg(any(target_os = "none", target_os = "hermit"))]
pub extern "C" fn __sys_free(ptr: *mut u8, size: usize, align: usize) {
	unsafe {
		let layout_res = Layout::from_size_align(size, align);
		if layout_res.is_err() || size == 0 {
			warn!(
				"__sys_free called with size {:#x}, align {:#x} is an invalid layout!",
				size, align
			);
			debug_assert!(layout_res.is_err(), "__sys_free error: Invalid layout");
			debug_assert_ne!(size, 0, "__sys_free error: size cannot be 0");
		} else {
			trace!(
				"sys_free: deallocate memory at {:#x} (size {:#x})",
				ptr as usize,
				size
			);
		}
		let layout = layout_res.unwrap();
		ALLOCATOR.dealloc(ptr, layout);
	}
}

#[cfg(any(target_os = "none", target_os = "hermit"))]
extern "C" {
	static mut __bss_start: usize;
}

/// Helper function to check if uhyve provide an IP device
#[cfg(feature = "newlib")]
fn has_ipdevice() -> bool {
	arch::x86_64::kernel::has_ipdevice()
}

/// Entry point of a kernel thread, which initialize the libos
#[cfg(any(target_os = "none", target_os = "hermit"))]
extern "C" fn initd(_arg: usize) {
	extern "C" {
		#[cfg(not(test))]
		fn runtime_entry(argc: i32, argv: *const *const u8, env: *const *const u8) -> !;
		#[cfg(feature = "newlib")]
		fn init_lwip();
	}

	// initialize LwIP library for newlib-based applications
	#[cfg(feature = "newlib")]
	unsafe {
		if has_ipdevice() {
			init_lwip();
		}
	}

	if environment::is_uhyve() {
		// Initialize the uhyve-net interface using the IP and gateway addresses specified in hcip, hcmask, hcgateway.
		info!("HermitCore is running on uhyve!");
	} else if !environment::is_single_kernel() {
		// Initialize the mmnif interface using static IPs in the range 192.168.28.x.
		info!("HermitCore is running side-by-side to Linux!");
	} else {
		info!("HermitCore is running on common system!");
	}

	// Initialize Drivers
	arch::init_drivers();

	syscalls::init();

	// Get the application arguments and environment variables.
	#[cfg(not(test))]
	let (argc, argv, environ) = syscalls::get_application_parameters();

	// give the IP thread time to initialize the network interface
	core_scheduler().reschedule();

	#[cfg(not(test))]
	unsafe {
		// And finally start the application.
		runtime_entry(argc, argv, environ)
	}
	#[cfg(test)]
	test_main();
}

#[cfg(feature = "smp")]
fn synch_all_cores() {
	static CORE_COUNTER: AtomicU32 = AtomicU32::new(0);

	CORE_COUNTER.fetch_add(1, Ordering::SeqCst);

	while CORE_COUNTER.load(Ordering::SeqCst) != get_processor_count() {
		spin_loop();
	}
}

/// Entry Point of HermitCore for the Boot Processor
#[cfg(any(target_os = "none", target_os = "hermit"))]
fn boot_processor_main() -> ! {
	// Initialize the kernel and hardware.
	arch::message_output_init();
	unsafe {
		logging::init();
	}

	info!("Welcome to HermitCore-rs {}", env!("CARGO_PKG_VERSION"));
	info!("Kernel starts at {:#x}", environment::get_base_address());
	info!("BSS starts at {:#x}", unsafe {
		&__bss_start as *const usize as usize
	});
	info!(
		"TLS starts at {:#x} (size {} Bytes)",
		environment::get_tls_start(),
		environment::get_tls_memsz()
	);
	#[cfg(target_arch = "aarch64")]
	{
		info!("The current hermit-kernel is only implemented up to this point on aarch64.");
		info!("Attempting to exit via QEMU.");
		info!("This requires that you passed the `-semihosting` option to QEMU.");
		let exit_handler = qemu_exit::AArch64::new();
		exit_handler.exit_success();
		loop {} /* Compiles up to here - loop prevents linker errors */
	}
	arch::boot_processor_init();
	scheduler::add_current_core();

	if environment::is_single_kernel() && !environment::is_uhyve() {
		arch::boot_application_processors();
	}

	#[cfg(feature = "smp")]
	synch_all_cores();

	#[cfg(feature = "pci")]
	info!("Compiled with PCI support");
	#[cfg(feature = "acpi")]
	info!("Compiled with ACPI support");
	#[cfg(feature = "fsgsbase")]
	info!("Compiled with FSGSBASE support");
	#[cfg(feature = "smp")]
	info!("Compiled with SMP support");

	// Start the initd task.
	scheduler::PerCoreScheduler::spawn(initd, 0, scheduler::task::NORMAL_PRIO, 0, USER_STACK_SIZE);

	let core_scheduler = core_scheduler();
	// Run the scheduler loop.
	core_scheduler.run();
}

/// Entry Point of HermitCore for an Application Processor
#[cfg(all(any(target_os = "none", target_os = "hermit"), feature = "smp"))]
fn application_processor_main() -> ! {
	arch::application_processor_init();
	scheduler::add_current_core();

	info!("Entering idle loop for application processor");

	synch_all_cores();

	let core_scheduler = core_scheduler();
	// Run the scheduler loop.
	core_scheduler.run();
}
