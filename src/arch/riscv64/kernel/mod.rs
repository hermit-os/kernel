pub mod core_local;
mod devicetree;
pub mod interrupts;
#[cfg(all(any(feature = "virtio", feature = "gem-net"), not(feature = "pci")))]
pub mod mmio;
#[cfg(feature = "pci")]
pub mod pci;
pub mod processor;
pub mod scheduler;
pub mod serial;
pub mod switch;
pub mod systemtime;
use alloc::vec::Vec;
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, Ordering};

use free_list::PageLayout;
use memory_addresses::VirtAddr;
use riscv::register::sstatus;

pub(crate) use self::processor::set_oneshot_timer;
use crate::arch::kernel::core_local::core_id;
pub use crate::arch::kernel::devicetree::init_drivers;
pub use crate::arch::kernel::interrupts::wakeup_core;
use crate::arch::kernel::processor::lsb;
use crate::config::KERNEL_STACK_SIZE;
use crate::env::{self, FdtStartInfo};
use crate::init_cell::InitCell;
use crate::mm::{FrameAlloc, PageRangeAllocator};

// Used to store information about available harts. The index of the hart in the vector
// represents its CpuId and does not need to match its hart_id
pub(crate) static HARTS_AVAILABLE: InitCell<Vec<usize>> = InitCell::new(Vec::new());

// Address of interrupt files for each hart index by hart_id.
// Use HARTS_AVAILABLE to map CpuId to hart_id.
pub(crate) static INTERRUPT_FILES: InitCell<Vec<VirtAddr>> = InitCell::new(Vec::new());

/// Kernel header to announce machine features
pub(crate) static CPU_ONLINE: AtomicU32 = AtomicU32::new(0);
pub(crate) static CURRENT_BOOT_ID: AtomicU32 = AtomicU32::new(0);
pub(crate) static CURRENT_STACK_ADDRESS: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
pub(crate) static HART_MASK: AtomicU64 = AtomicU64::new(0);
#[cfg_attr(not(any(feature = "hermit-entry", feature = "smp")), expect(dead_code))]
pub(crate) static NUM_CPUS: AtomicU32 = AtomicU32::new(0);

// FUNCTIONS

#[cfg(feature = "smp")]
pub fn get_possible_cpus() -> u32 {
	NUM_CPUS.load(Ordering::Relaxed)
}

#[cfg(feature = "smp")]
pub fn get_processor_count() -> u32 {
	CPU_ONLINE.load(Ordering::Relaxed)
}

#[cfg(not(feature = "smp"))]
pub fn get_processor_count() -> u32 {
	1
}

pub fn get_hart_mask() -> u64 {
	HART_MASK.load(Ordering::Relaxed)
}

pub fn get_timebase_freq() -> u64 {
	let fdt = env::start_info().fdt().unwrap();

	// Get timebase-freq
	let cpus_node = fdt
		.find_node("/cpus")
		.expect("cpus node missing or invalid");
	cpus_node
		.property("timebase-frequency")
		.expect("timebase-frequency node not found in /cpus")
		.as_usize()
		.unwrap() as u64
}

pub fn get_current_boot_id() -> u32 {
	CURRENT_BOOT_ID.load(Ordering::Relaxed)
}

/// Real Boot Processor initialization as soon as we have put the first Welcome message on the screen.
pub fn boot_processor_init() {
	crate::mm::init();
	crate::mm::print_information();
	env::init();
	devicetree::init_interrupt_controller();
	interrupts::install();
	#[cfg(feature = "pci")]
	pci::init();

	finish_processor_init();
}

/// Application Processor initialization
#[cfg(feature = "smp")]
pub fn application_processor_init() {
	use crate::arch::kernel::core_local::CoreLocal;

	unsafe {
		super::mm::paging::enable_page_table();
	}
	CoreLocal::install();
	interrupts::install();
	finish_processor_init();
}

fn finish_processor_init() {
	unsafe {
		sstatus::set_fs(sstatus::FS::Initial);
	}
	trace!("SSTATUS FS: {:?}", sstatus::read().fs());

	let current_hart_id = get_current_boot_id() as usize;

	// Add hart to HARTS_AVAILABLE, the hart id is stored in current_boot_id
	HARTS_AVAILABLE.with(|harts_available| harts_available.unwrap().push(current_hart_id));
	info!("Initialized CPU with hart_id {current_hart_id}");

	crate::scheduler::add_current_core();

	// Remove current hart from the hart_mask
	let new_hart_mask = get_hart_mask() & (u64::MAX - (1 << current_hart_id));
	HART_MASK.store(new_hart_mask, Ordering::Relaxed);
}

pub fn boot_next_processor() {
	let new_hart_mask = HART_MASK.load(Ordering::Relaxed);
	debug!("HART_MASK = {new_hart_mask:#x}");

	let next_hart_index = lsb(new_hart_mask);

	let Some(next_hart_id) = next_hart_index else {
		info!("All processors are initialized");
		CPU_ONLINE.fetch_add(1, Ordering::Release);
		return;
	};

	{
		debug!("Allocating stack for hard_id {next_hart_id}");
		let frame_layout = PageLayout::from_size(KERNEL_STACK_SIZE).unwrap();
		let frame_range =
			FrameAlloc::allocate(frame_layout).expect("Failed to allocate boot stack for new core");
		let stack = ptr::with_exposed_provenance_mut(frame_range.start());
		CURRENT_STACK_ADDRESS.store(stack, Ordering::Relaxed);
	}

	info!(
		"Starting CPU {} with hart_id {}",
		core_id() + 1,
		next_hart_id
	);

	// TODO: Old: Changing cpu_online will cause uhyve to start the next processor
	CPU_ONLINE.fetch_add(1, Ordering::Release);

	#[cfg(feature = "uhyve")]
	use env::UhyveStartInfo;

	#[allow(clippy::needless_return)]
	#[cfg(feature = "uhyve")]
	if env::start_info().is_uhyve() {
		return;
	}

	#[cfg(feature = "smp")]
	{
		//When running bare-metal/QEMU we use the firmware to start the next hart
		let start_addr = (crate::arch::start::smp::smp_start as *const ()).expose_provenance();
		sbi_rt::hart_start(next_hart_id as usize, start_addr, 0).unwrap();
	}
}

pub fn print_statistics() {
	interrupts::print_statistics();
}
