use crate::arch::kernel;
use crate::arch::kernel::core_local::{core_id, core_scheduler};
use crate::arch::kernel::interrupts;
use crate::env::{self, StartInfo};
use crate::scheduler::{PerCoreScheduler, PerCoreSchedulerExt};
use crate::{console, drivers, executor, fs, logging, mm, scheduler, syscalls};

mod built_info {
	include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

hermit_entry::define_abi_tag!();

hermit_entry::define_entry_version!();

#[cfg(test)]
#[unsafe(no_mangle)]
extern "C" fn runtime_entry(_argc: i32, _argv: *const *const u8, _env: *const *const u8) -> ! {
	println!("Executing hermit unittests. Any arguments are dropped");
	crate::test_main();
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

#[test_case]
fn trivial_test() {
	println!("Test test test");
	panic!("Test called");
}

/// Entry point of a kernel thread, which initialize the libos
extern "C" fn initd(_arg: usize) {
	unsafe extern "C" {
		#[cfg(all(not(test), not(any(feature = "nostd", feature = "common-os"))))]
		fn runtime_entry(argc: i32, argv: *const *const u8, env: *const *const u8) -> !;
		#[cfg(all(not(test), any(feature = "nostd", feature = "common-os")))]
		fn main(argc: i32, argv: *const *const u8, env: *const *const u8);
	}

	// Initialize Drivers
	drivers::init();
	// The filesystem needs to be initialized before network to allow writing packet captures to a file.
	fs::init();
	executor::init();

	syscalls::init();
	#[cfg(feature = "shell")]
	crate::shell::init();

	// Get the application arguments and environment variables.
	#[cfg(not(test))]
	let (argc, argv, environ) = syscalls::get_application_parameters();

	// give the IP thread time to initialize the network interface
	core_scheduler().reschedule();

	if cfg!(feature = "warn-prebuilt") {
		warn!("This is a prebuilt Hermit kernel.");
		warn!("For non-default device drivers and features, consider building a custom kernel.");
	}

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
	crate::test_main();
}

#[cfg(feature = "smp")]
fn synch_all_cores() {
	use core::hint;
	use core::sync::atomic::{AtomicU32, Ordering};

	static CORE_COUNTER: AtomicU32 = AtomicU32::new(0);

	CORE_COUNTER.fetch_add(1, Ordering::SeqCst);

	let possible_cpus = kernel::get_possible_cpus();
	while CORE_COUNTER.load(Ordering::SeqCst) != possible_cpus {
		hint::spin_loop();
	}
}

/// Entry Point of Hermit for the Boot Processor
pub fn boot_processor_main() -> ! {
	use crate::config::USER_STACK_SIZE;

	// Initialize the kernel and hardware.
	mm::claim_initial_heap();
	hermit_sync::Lazy::force(&console::CONSOLE);
	env::init();
	unsafe {
		logging::init();
	}

	info!("Welcome to Hermit {}", env!("CARGO_PKG_VERSION"));
	if let Some(git_version) = built_info::GIT_VERSION {
		let dirty = if built_info::GIT_DIRTY == Some(true) {
			" (dirty)"
		} else {
			""
		};

		let opt_level = if built_info::OPT_LEVEL == "3" {
			format_args!("")
		} else {
			format_args!(" (opt-level={})", built_info::OPT_LEVEL)
		};

		info!("Git version: {git_version}{dirty}{opt_level}");
	}
	let arch = built_info::TARGET.split_once('-').unwrap().0;
	info!("Architecture: {arch}");
	info!("Enabled features: {}", built_info::FEATURES_LOWERCASE_STR);
	info!("Built on {}", built_info::BUILT_TIME_UTC);

	info!("Executable start: {:p}", elf_symbols::executable_start());
	info!("ELF header:       {:p}", elf_symbols::elf_header());
	info!("Text segment end: {:p}", elf_symbols::text_end());
	info!("Data segment end: {:p}", elf_symbols::data_end());
	info!("Executable end:   {:p}", elf_symbols::executable_end());

	info!("{}", env::start_info().display());

	kernel::boot_processor_init();

	#[cfg(not(target_arch = "riscv64"))]
	scheduler::add_current_core();
	interrupts::enable();

	kernel::boot_next_processor();

	#[cfg(feature = "smp")]
	synch_all_cores();

	#[cfg(feature = "pci")]
	drivers::pci::print_information();

	// Start the initd task.
	unsafe { PerCoreScheduler::spawn(initd, 0, scheduler::task::NORMAL_PRIO, 0, USER_STACK_SIZE) };

	// Run the scheduler loop.
	PerCoreScheduler::run();
}

/// Entry Point of Hermit for an Application Processor
#[cfg(feature = "smp")]
pub fn application_processor_main() -> ! {
	kernel::application_processor_init();
	#[cfg(not(target_arch = "riscv64"))]
	scheduler::add_current_core();
	interrupts::enable();
	kernel::boot_next_processor();

	debug!("Entering idle loop for application processor");

	synch_all_cores();
	executor::init();

	// Run the scheduler loop.
	PerCoreScheduler::run();
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
	let core_id = core_id();
	panic_println!("[{core_id}][PANIC] {info}\n");

	scheduler::shutdown(1);
}
