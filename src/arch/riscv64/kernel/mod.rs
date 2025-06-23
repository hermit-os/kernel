pub mod core_local;
mod devicetree;
pub mod interrupts;
#[cfg(all(feature = "tcp", not(feature = "pci")))]
pub mod mmio;
#[cfg(feature = "pci")]
pub mod pci;
pub mod processor;
pub mod scheduler;
mod start;
pub mod switch;
pub mod systemtime;

use alloc::vec::Vec;
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, Ordering};
use core::task::Waker;

use fdt::Fdt;
use memory_addresses::{PhysAddr, VirtAddr};
use riscv::register::sstatus;

use crate::arch::riscv64::kernel::core_local::{CoreLocal, core_id};
pub use crate::arch::riscv64::kernel::devicetree::init_drivers;
use crate::arch::riscv64::kernel::processor::lsb;
use crate::config::KERNEL_STACK_SIZE;
use crate::env;
use crate::init_cell::InitCell;
use crate::mm::physicalmem;

pub(crate) struct Console {}

impl Console {
	pub fn new() -> Self {
		CoreLocal::install();

		Self {}
	}

	pub fn write(&mut self, buf: &[u8]) {
		for byte in buf {
			sbi_rt::console_write_byte(*byte);
		}
	}

	pub fn read(&mut self) -> Option<u8> {
		None
	}

	pub fn is_empty(&self) -> bool {
		true
	}

	pub fn register_waker(&mut self, _waker: &Waker) {}
}

impl Default for Console {
	fn default() -> Self {
		Self::new()
	}
}

// Used to store information about available harts. The index of the hart in the vector
// represents its CpuId and does not need to match its hart_id
pub(crate) static HARTS_AVAILABLE: InitCell<Vec<usize>> = InitCell::new(Vec::new());

/// Kernel header to announce machine features
static CPU_ONLINE: AtomicU32 = AtomicU32::new(0);
static CURRENT_BOOT_ID: AtomicU32 = AtomicU32::new(0);
static CURRENT_STACK_ADDRESS: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
static HART_MASK: AtomicU64 = AtomicU64::new(0);
static NUM_CPUS: AtomicU32 = AtomicU32::new(0);

// FUNCTIONS

pub fn is_uhyve_with_pci() -> bool {
	false
}

pub fn get_ram_address() -> PhysAddr {
	PhysAddr::new(env::boot_info().hardware_info.phys_addr_range.start)
}

pub fn get_image_size() -> usize {
	(env::boot_info().load_info.kernel_image_addr_range.end
		- env::boot_info().load_info.kernel_image_addr_range.start) as usize
}

pub fn get_limit() -> usize {
	(env::boot_info().hardware_info.phys_addr_range.end
		- env::boot_info().hardware_info.phys_addr_range.start) as usize
}

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

pub fn get_base_address() -> VirtAddr {
	VirtAddr::new(env::boot_info().load_info.kernel_image_addr_range.start)
}

pub fn args() -> Option<&'static str> {
	None
}

pub fn get_dtb_ptr() -> *const u8 {
	env::boot_info().hardware_info.device_tree.unwrap().get() as _
}

pub fn get_hart_mask() -> u64 {
	HART_MASK.load(Ordering::Relaxed)
}

pub fn get_timebase_freq() -> u64 {
	unsafe {
		let fdt = Fdt::from_ptr(get_dtb_ptr()).expect("FDT is invalid");

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
}

pub fn get_current_boot_id() -> u32 {
	CURRENT_BOOT_ID.load(Ordering::Relaxed)
}

/// Real Boot Processor initialization as soon as we have put the first Welcome message on the screen.
pub fn boot_processor_init() {
	devicetree::init();
	crate::mm::init();
	crate::mm::print_information();
	env::init();
	interrupts::install();

	finish_processor_init();
}

/// Application Processor initialization
#[cfg(feature = "smp")]
pub fn application_processor_init() {
	super::mm::paging::init_application_processor();
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

	debug!("Current HART_MASK: 0x{new_hart_mask:x}");
	let next_hart_index = lsb(new_hart_mask);

	if let Some(next_hart_id) = next_hart_index {
		debug!("Preparing to start HART {next_hart_id}");

		{
			let stack = physicalmem::allocate(KERNEL_STACK_SIZE)
				.expect("Failed to allocate boot stack for new core");
			CURRENT_STACK_ADDRESS.store(stack.as_usize() as _, Ordering::Relaxed);
		}

		info!(
			"Starting CPU {} with hart_id {}",
			core_id() + 1,
			next_hart_id
		);

		// TODO: Old: Changing cpu_online will cause uhyve to start the next processor
		CPU_ONLINE.fetch_add(1, Ordering::Release);

		//When running bare-metal/QEMU we use the firmware to start the next hart
		if !env::is_uhyve() {
			sbi_rt::hart_start(next_hart_id as usize, start::_start as usize, 0).unwrap();
		}
	} else {
		info!("All processors are initialized");
		CPU_ONLINE.fetch_add(1, Ordering::Release);
	}
}

pub fn print_statistics() {
	interrupts::print_statistics();
}
