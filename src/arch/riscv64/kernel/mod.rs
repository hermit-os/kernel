pub mod core_local;
mod devicetree;
pub mod interrupts;
#[cfg(all(
	any(feature = "virtio-net", feature = "console", feature = "gem-net"),
	not(feature = "pci"),
))]
pub mod mmio;
#[cfg(feature = "pci")]
pub mod pci;
pub mod processor;
pub mod scheduler;
pub mod serial;
mod start;
pub mod switch;
pub mod systemtime;
use alloc::vec::Vec;
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, Ordering};

use fdt::Fdt;
use free_list::PageLayout;
use memory_addresses::{PhysAddr, VirtAddr};
use riscv::register::sstatus;

use crate::arch::riscv64::kernel::core_local::core_id;
pub use crate::arch::riscv64::kernel::devicetree::init_drivers;
use crate::arch::riscv64::kernel::processor::lsb;
use crate::config::KERNEL_STACK_SIZE;
use crate::env;
use crate::init_cell::InitCell;
use crate::mm::physicalmem::PHYSICAL_FREE_LIST;

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
	crate::CoreLocal::install();
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
	debug!("HART: New mask is equal to {new_hart_mask:#x}");

	let next_hart_index = lsb(new_hart_mask);

	if let Some(next_hart_id) = next_hart_index {
		{
			debug!("HART: Allocating stack for ID {next_hart_id}");
			let frame_layout = PageLayout::from_size(KERNEL_STACK_SIZE).unwrap();
			let frame_range = PHYSICAL_FREE_LIST.lock().allocate(frame_layout).expect(
				"HART: Unable to allocate boot stack for new core with hart id: {next_hart_id})",
			);
			let stack = PhysAddr::from(frame_range.start());
			CURRENT_STACK_ADDRESS.store(stack.as_usize() as _, Ordering::Relaxed);
		}

		debug!(
			"CPU {}: Start with with hart_id {next_hart_id}",
			core_id() + 1
		);

		// TODO: Old: Changing cpu_online will cause uhyve to start the next processor
		CPU_ONLINE.fetch_add(1, Ordering::Release);

		// When running bare-metal/QEMU, we use the firmware to start the next hart
		if !env::is_uhyve() {
			sbi_rt::hart_start(next_hart_id as usize, start::_start as usize, 0).unwrap();
		}
	} else {
		debug!("All processors have been initialized.");
		CPU_ONLINE.fetch_add(1, Ordering::Release);
	}
}

pub fn print_statistics() {
	interrupts::print_statistics();
}
