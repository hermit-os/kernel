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
use core::arch::global_asm;
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, Ordering};

use fdt::Fdt;
use hermit_entry::boot_info::{BootInfo, RawBootInfo};
use hermit_sync::OnceCell;
use riscv::register::sstatus;

use crate::arch::riscv64::kernel::core_local::{core_id, CoreLocal};
pub use crate::arch::riscv64::kernel::devicetree::init_drivers;
use crate::arch::riscv64::kernel::processor::lsb;
use crate::arch::riscv64::mm::{physicalmem, PhysAddr, VirtAddr};
use crate::config::KERNEL_STACK_SIZE;
use crate::env;

global_asm!(include_str!("setjmp.s"));
global_asm!(include_str!("longjmp.s"));

// Used to store information about available harts. The index of the hart in the vector
// represents its CpuId and does not need to match its hart_id
pub static mut HARTS_AVAILABLE: Vec<usize> = Vec::new();

/// Kernel header to announce machine features
static BOOT_INFO: OnceCell<BootInfo> = OnceCell::new();
static RAW_BOOT_INFO: AtomicPtr<RawBootInfo> = AtomicPtr::new(ptr::null_mut());
static CPU_ONLINE: AtomicU32 = AtomicU32::new(0);
static CURRENT_BOOT_ID: AtomicU32 = AtomicU32::new(0);
static CURRENT_STACK_ADDRESS: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
static HART_MASK: AtomicU64 = AtomicU64::new(0);
static NUM_CPUS: AtomicU32 = AtomicU32::new(0);

// FUNCTIONS

pub fn boot_info() -> &'static BootInfo {
	BOOT_INFO.get().unwrap()
}

pub fn is_uhyve_with_pci() -> bool {
	false
}

pub fn get_ram_address() -> PhysAddr {
	PhysAddr(boot_info().hardware_info.phys_addr_range.start)
}

pub fn get_image_size() -> usize {
	(boot_info().load_info.kernel_image_addr_range.end
		- boot_info().load_info.kernel_image_addr_range.start) as usize
}

pub fn get_limit() -> usize {
	(boot_info().hardware_info.phys_addr_range.end
		- boot_info().hardware_info.phys_addr_range.start) as usize
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
	VirtAddr(boot_info().load_info.kernel_image_addr_range.start)
}

pub fn args() -> Option<&'static str> {
	unsafe {
		let fdt = Fdt::from_ptr(get_dtb_ptr()).expect("FDT is invalid");
		fdt.chosen().bootargs()
	}
}

pub fn get_dtb_ptr() -> *const u8 {
	boot_info().hardware_info.device_tree.unwrap().get() as _
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

/// Earliest initialization function called by the Boot Processor.
pub fn message_output_init() {
	CoreLocal::install();
}

pub fn output_message_buf(buf: &[u8]) {
	for byte in buf {
		sbi_rt::console_write_byte(*byte);
	}
}

/// Real Boot Processor initialization as soon as we have put the first Welcome message on the screen.
pub fn boot_processor_init() {
	devicetree::init();
	crate::mm::init();
	crate::mm::print_information();
	env::init();
	interrupts::install();

	finish_processor_init();
	interrupts::enable();
}

/// Boots all available Application Processors on bare-metal or QEMU.
/// Called after the Boot Processor has been fully initialized along with its scheduler.
pub fn boot_application_processors() {
	// Nothing to do here yet.
}

/// Application Processor initialization
#[cfg(feature = "smp")]
pub fn application_processor_init() {
	super::mm::paging::init_application_processor();
	CoreLocal::install();
	interrupts::install();
	finish_processor_init();
	interrupts::enable();
}

fn finish_processor_init() {
	unsafe {
		sstatus::set_fs(sstatus::FS::Initial);
	}
	trace!("SSTATUS FS: {:?}", sstatus::read().fs());

	let current_hart_id = get_current_boot_id() as usize;

	unsafe {
		// Add hart to HARTS_AVAILABLE, the hart id is stored in current_boot_id
		HARTS_AVAILABLE.push(current_hart_id);
		info!(
			"Initialized CPU with hart_id {}",
			HARTS_AVAILABLE[core_local::core_id() as usize]
		);
	}

	crate::scheduler::add_current_core();

	// Remove current hart from the hart_mask
	let new_hart_mask = get_hart_mask() & (u64::MAX - (1 << current_hart_id));
	HART_MASK.store(new_hart_mask, Ordering::Relaxed);

	let next_hart_index = lsb(new_hart_mask);

	if let Some(next_hart_id) = next_hart_index {
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
			sbi_rt::hart_start(
				next_hart_id as usize,
				start::_start as usize,
				RAW_BOOT_INFO.load(Ordering::Relaxed) as usize,
			)
			.unwrap();
		}
	} else {
		info!("All processors are initialized");
		CPU_ONLINE.fetch_add(1, Ordering::Release);
	}
}

pub fn print_statistics() {}
