//! First version is derived and adapted for Hermit from
//! Philipp Oppermann's excellent series of blog posts (<http://blog.phil-opp.com/>)
//! and Eric Kidd's toy OS (<https://github.com/emk/toyos-rs>).

#![allow(clippy::missing_safety_doc)]
#![cfg_attr(
	any(target_arch = "aarch64", target_arch = "riscv64"),
	allow(incomplete_features)
)]
#![cfg_attr(target_arch = "x86_64", feature(abi_x86_interrupt))]
#![feature(allocator_api)]
#![feature(linkage)]
#![feature(linked_list_cursors)]
#![feature(map_try_insert)]
#![feature(maybe_uninit_as_bytes)]
#![feature(maybe_uninit_slice)]
#![feature(maybe_uninit_write_slice)]
#![feature(never_type)]
#![feature(slice_from_ptr_range)]
#![feature(slice_ptr_get)]
#![cfg_attr(
	any(target_arch = "aarch64", target_arch = "riscv64"),
	feature(specialization)
)]
#![feature(thread_local)]
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", feature(custom_test_frameworks))]
#![cfg_attr(all(target_os = "none", test), test_runner(crate::test_runner))]
#![cfg_attr(
	all(target_os = "none", test),
	reexport_test_harness_main = "test_main"
)]
#![cfg_attr(all(target_os = "none", test), no_main)]
#![cfg_attr(feature = "balloon", feature(iter_collect_into, iter_array_chunks))]

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

#[cfg(feature = "smp")]
use core::hint::spin_loop;
#[cfg(feature = "smp")]
use core::sync::atomic::{AtomicU32, Ordering};

use arch::core_local::*;

pub(crate) use crate::arch::*;
pub use crate::config::DEFAULT_STACK_SIZE;
pub(crate) use crate::config::*;
pub use crate::fs::create_file;
use crate::kernel::is_uhyve_with_pci;
use crate::scheduler::{PerCoreScheduler, PerCoreSchedulerExt};

#[macro_use]
mod macros;

#[macro_use]
mod logging;

pub mod arch;
mod config;
pub mod console;
mod drivers;
mod entropy;
mod env;
pub mod errno;
mod executor;
pub mod fd;
pub mod fs;
mod init_cell;
pub mod io;
pub mod mm;
pub mod scheduler;
#[cfg(all(feature = "shell", target_arch = "x86_64"))]
mod shell;
mod synch;
pub mod syscalls;
pub mod time;

hermit_entry::define_abi_tag!();

#[cfg(target_os = "none")]
hermit_entry::define_entry_version!();

#[cfg(test)]
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
extern "C" fn runtime_entry(_argc: i32, _argv: *const *const u8, _env: *const *const u8) -> ! {
	println!("Executing hermit unittests. Any arguments are dropped");
	test_main();
	core_scheduler().exit(0)
}

//https://github.com/rust-lang/rust/issues/50297#issuecomment-524180479
#[cfg(test)]
pub fn test_runner(tests: &[&dyn Fn()]) {
	println!("Running {} tests", tests.len());
	for test in tests {
		test();
	}
	core_scheduler().exit(0)
}

#[cfg(target_os = "none")]
#[test_case]
fn trivial_test() {
	println!("Test test test");
	panic!("Test called");
}

/// Entry point of a kernel thread, which initialize the libos
#[cfg(target_os = "none")]
extern "C" fn initd(_arg: usize) {
	unsafe extern "C" {
		#[cfg(all(not(test), not(any(feature = "nostd", feature = "common-os"))))]
		fn runtime_entry(argc: i32, argv: *const *const u8, env: *const *const u8) -> !;
		#[cfg(all(not(test), any(feature = "nostd", feature = "common-os")))]
		fn main(argc: i32, argv: *const *const u8, env: *const *const u8);
	}

	if env::is_uhyve() {
		info!("Hermit is running on uhyve!");
	} else {
		info!("Hermit is running on common system!");
	}

	// Initialize Drivers
	drivers::init();
	crate::executor::init();

	syscalls::init();
	fs::init();
	#[cfg(all(feature = "shell", target_arch = "x86_64"))]
	shell::init();

	// Get the application arguments and environment variables.
	#[cfg(not(test))]
	let (argc, argv, environ) = syscalls::get_application_parameters();

	// give the IP thread time to initialize the network interface
	core_scheduler().reschedule();

	info!("Jumping into application");

	#[cfg(not(test))]
	unsafe {
		// And finally start the application.
		#[cfg(all(not(test), not(any(feature = "nostd", feature = "common-os"))))]
		runtime_entry(argc, argv, environ);
		#[cfg(all(not(test), any(feature = "nostd", feature = "common-os")))]
		main(argc, argv, environ);
	}
	#[cfg(test)]
	test_main();
}

#[cfg(feature = "smp")]
fn synch_all_cores() {
	static CORE_COUNTER: AtomicU32 = AtomicU32::new(0);

	CORE_COUNTER.fetch_add(1, Ordering::SeqCst);

	let possible_cpus = kernel::get_possible_cpus();
	while CORE_COUNTER.load(Ordering::SeqCst) != possible_cpus {
		spin_loop();
	}
}

/// Entry Point of Hermit for the Boot Processor
#[cfg(target_os = "none")]
fn boot_processor_main() -> ! {
	// Initialize the kernel and hardware.
	hermit_sync::Lazy::force(&console::CONSOLE);
	unsafe {
		logging::init();
	}

	info!("Welcome to Hermit {}", env!("CARGO_PKG_VERSION"));
	info!("Kernel starts at {:p}", env::get_base_address());

	if let Some(fdt) = env::fdt() {
		info!("FDT:\n{fdt:#?}");
	}

	unsafe extern "C" {
		static mut __bss_start: u8;
	}
	let bss_ptr = core::ptr::addr_of_mut!(__bss_start);
	info!("BSS starts at {bss_ptr:p}");
	info!("tls_info = {:#x?}", env::boot_info().load_info.tls_info);
	arch::boot_processor_init();

	#[cfg(not(target_arch = "riscv64"))]
	scheduler::add_current_core();
	interrupts::enable();

	arch::kernel::boot_next_processor();

	#[cfg(feature = "smp")]
	synch_all_cores();

	#[cfg(feature = "pci")]
	info!("Compiled with PCI support");
	#[cfg(all(feature = "acpi", target_arch = "x86_64"))]
	info!("Compiled with ACPI support");
	#[cfg(all(feature = "fsgsbase", target_arch = "x86_64"))]
	info!("Compiled with FSGSBASE support");
	#[cfg(feature = "smp")]
	info!("Compiled with SMP support");

	if is_uhyve_with_pci() || !env::is_uhyve() {
		#[cfg(feature = "pci")]
		crate::drivers::pci::print_information();
	}

	// Start the initd task.
	unsafe {
		scheduler::PerCoreScheduler::spawn(
			initd,
			0,
			scheduler::task::NORMAL_PRIO,
			0,
			USER_STACK_SIZE,
		)
	};

	// Run the scheduler loop.
	PerCoreScheduler::run();
}

/// Entry Point of Hermit for an Application Processor
#[cfg(all(target_os = "none", feature = "smp"))]
fn application_processor_main() -> ! {
	arch::application_processor_init();
	#[cfg(not(target_arch = "riscv64"))]
	scheduler::add_current_core();
	interrupts::enable();
	arch::kernel::boot_next_processor();

	debug!("Entering idle loop for application processor");

	synch_all_cores();
	crate::executor::init();

	// Run the scheduler loop.
	PerCoreScheduler::run();
}

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
	let core_id = crate::arch::core_local::core_id();
	panic_println!("[{core_id}][PANIC] {info}\n");

	crate::scheduler::shutdown(1);
}
