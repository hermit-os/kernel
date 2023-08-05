//! First version is derived and adapted for HermitCore from
//! Philipp Oppermann's excellent series of blog posts (<http://blog.phil-opp.com/>)
//! and Eric Kidd's toy OS (<https://github.com/emk/toyos-rs>).

#![warn(rust_2018_idioms)]
#![warn(unsafe_op_in_unsafe_fn)]
#![warn(clippy::uninlined_format_args)]
#![warn(clippy::transmute_ptr_to_ptr)]
#![allow(clippy::missing_safety_doc)]
#![cfg_attr(target_arch = "aarch64", allow(incomplete_features))]
#![cfg_attr(target_arch = "x86_64", feature(abi_x86_interrupt))]
#![feature(allocator_api)]
#![feature(asm_const)]
#![feature(core_intrinsics)]
#![feature(linked_list_cursors)]
#![feature(maybe_uninit_slice)]
#![feature(naked_functions)]
#![feature(pointer_byte_offsets)]
#![feature(pointer_is_aligned)]
#![feature(noop_waker)]
#![cfg_attr(target_arch = "aarch64", feature(specialization))]
#![feature(strict_provenance)]
#![feature(thread_local)]
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", feature(custom_test_frameworks))]
#![cfg_attr(all(target_os = "none", test), test_runner(crate::test_runner))]
#![cfg_attr(
	all(target_os = "none", test),
	reexport_test_harness_main = "test_main"
)]
#![cfg_attr(all(target_os = "none", test), no_main)]

// EXTERNAL CRATES
#[macro_use]
extern crate alloc;
#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate log;
#[cfg(not(target_os = "none"))]
#[macro_use]
extern crate std;
#[macro_use]
extern crate num_derive;

use alloc::alloc::Layout;
use core::alloc::GlobalAlloc;
#[cfg(feature = "smp")]
use core::hint::spin_loop;
#[cfg(feature = "smp")]
use core::sync::atomic::{AtomicU32, Ordering};

use arch::core_local::*;
// Used for integration test status.
#[doc(hidden)]
pub use env::is_uhyve as _is_uhyve;
use mm::allocator::LockedAllocator;

pub(crate) use crate::arch::*;
pub(crate) use crate::config::*;
use crate::kernel::is_uhyve_with_pci;
pub use crate::syscalls::*;

#[macro_use]
mod macros;

#[macro_use]
mod logging;

mod arch;
mod config;
mod console;
mod drivers;
mod entropy;
mod env;
pub mod errno;
mod executor;
pub(crate) mod fd;
pub(crate) mod fs;
mod mm;
mod scheduler;
mod synch;
mod syscalls;

hermit_entry::define_entry_version!();

#[doc(hidden)]
pub fn _print(args: ::core::fmt::Arguments<'_>) {
	use core::fmt::Write;
	crate::console::CONSOLE.lock().write_fmt(args).unwrap();
}

#[cfg(test)]
#[cfg(target_os = "none")]
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

#[cfg(target_os = "none")]
#[test_case]
fn trivial_test() {
	println!("Test test test");
	panic!("Test called");
}

#[cfg(target_os = "none")]
#[global_allocator]
static ALLOCATOR: LockedAllocator = LockedAllocator::empty();

/// Interface to allocate memory from system heap
///
/// # Errors
/// Returning a null pointer indicates that either memory is exhausted or
/// `size` and `align` do not meet this allocator's size or alignment constraints.
///
#[cfg(target_os = "none")]
pub(crate) extern "C" fn __sys_malloc(size: usize, align: usize) -> *mut u8 {
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
		"__sys_malloc: allocate memory at {:p} (size {:#x}, align {:#x})",
		ptr,
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
#[cfg(target_os = "none")]
pub(crate) extern "C" fn __sys_realloc(
	ptr: *mut u8,
	size: usize,
	align: usize,
	new_size: usize,
) -> *mut u8 {
	unsafe {
		let layout_res = Layout::from_size_align(size, align);
		if layout_res.is_err() || size == 0 || new_size == 0 {
			warn!(
			"__sys_realloc called with ptr {:p}, size {:#x}, align {:#x}, new_size {:#x} is an invalid layout!",
			ptr, size, align, new_size
		);
			return core::ptr::null::<*mut u8>() as *mut u8;
		}
		let layout = layout_res.unwrap();
		let new_ptr = ALLOCATOR.realloc(ptr, layout, new_size);

		if new_ptr.is_null() {
			debug!(
			"__sys_realloc failed to resize ptr {:p} with size {:#x}, align {:#x}, new_size {:#x} !",
			ptr, size, align, new_size
		);
		} else {
			trace!(
				"__sys_realloc: resized memory at {:p}, new address {:p}",
				ptr,
				new_ptr
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
#[cfg(target_os = "none")]
pub(crate) extern "C" fn __sys_free(ptr: *mut u8, size: usize, align: usize) {
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
				"sys_free: deallocate memory at {:p} (size {:#x})",
				ptr,
				size
			);
		}
		let layout = layout_res.unwrap();
		ALLOCATOR.dealloc(ptr, layout);
	}
}

/// Entry point of a kernel thread, which initialize the libos
#[cfg(target_os = "none")]
extern "C" fn initd(_arg: usize) {
	extern "C" {
		#[cfg(not(test))]
		fn runtime_entry(argc: i32, argv: *const *const u8, env: *const *const u8) -> !;
		#[cfg(feature = "newlib")]
		fn init_lwip();
		#[cfg(feature = "newlib")]
		fn init_rtl8139_netif(freq: u32) -> i32;
	}

	if !env::is_uhyve() {
		// initialize LwIP library for newlib-based applications
		#[cfg(feature = "newlib")]
		unsafe {
			init_lwip();
			init_rtl8139_netif(processor::get_frequency() as u32);
		}

		info!("HermitCore is running on common system!");
	} else {
		info!("HermitCore is running on uhyve!");
	}

	// Initialize Drivers
	arch::init_drivers();
	crate::executor::init();

	syscalls::init();
	fd::init();
	fs::init();

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

	while CORE_COUNTER.load(Ordering::SeqCst) != kernel::get_possible_cpus() {
		spin_loop();
	}
}

/// Entry Point of HermitCore for the Boot Processor
#[cfg(target_os = "none")]
fn boot_processor_main() -> ! {
	// Initialize the kernel and hardware.
	arch::message_output_init();
	unsafe {
		logging::init();
	}

	info!("Welcome to HermitCore-rs {}", env!("CARGO_PKG_VERSION"));
	info!("Kernel starts at {:p}", env::get_base_address());

	extern "C" {
		static mut __bss_start: u8;
	}
	info!("BSS starts at {:p}", unsafe {
		core::ptr::addr_of_mut!(__bss_start)
	});
	info!(
		"TLS starts at {:p} (size {} Bytes)",
		env::get_tls_start(),
		env::get_tls_memsz()
	);

	arch::boot_processor_init();
	scheduler::add_current_core();

	if !env::is_uhyve() {
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

	if is_uhyve_with_pci() || !env::is_uhyve() {
		#[cfg(feature = "pci")]
		crate::drivers::pci::print_information();
	}

	// Start the initd task.
	scheduler::PerCoreScheduler::spawn(initd, 0, scheduler::task::NORMAL_PRIO, 0, USER_STACK_SIZE);

	let core_scheduler = core_scheduler();
	// Run the scheduler loop.
	core_scheduler.run();
}

/// Entry Point of HermitCore for an Application Processor
#[cfg(all(target_os = "none", feature = "smp"))]
fn application_processor_main() -> ! {
	arch::application_processor_init();
	scheduler::add_current_core();

	info!("Entering idle loop for application processor");

	synch_all_cores();
	crate::executor::init();

	let core_scheduler = core_scheduler();
	// Run the scheduler loop.
	core_scheduler.run();
}

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
	let core_id = crate::arch::core_local::core_id();
	println!("[{core_id}][PANIC] {info}");

	crate::__sys_shutdown(1);
}
