#[cfg(feature = "common-os")]
use core::arch::asm;
use core::arch::global_asm;
use core::num::NonZeroU64;
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use fdt::Fdt;
use hermit_entry::boot_info::{BootInfo, PlatformInfo, RawBootInfo};
use hermit_sync::InterruptSpinMutex;
use x86::controlregs::{cr0, cr0_write, cr4, Cr0};

use self::serial::SerialPort;
use crate::arch::mm::{PhysAddr, VirtAddr};
use crate::arch::x86_64::kernel::core_local::*;
use crate::env::{self, is_uhyve};

#[cfg(feature = "acpi")]
pub mod acpi;
pub mod apic;
pub mod core_local;
pub mod gdt;
pub mod interrupts;
#[cfg(all(not(feature = "pci"), any(feature = "tcp", feature = "udp")))]
pub mod mmio;
#[cfg(feature = "pci")]
pub mod pci;
pub mod pic;
pub mod pit;
pub mod processor;
pub mod scheduler;
pub mod serial;
#[cfg(target_os = "none")]
mod start;
pub mod switch;
#[cfg(feature = "common-os")]
mod syscall;
pub(crate) mod systemtime;
#[cfg(feature = "vga")]
mod vga;

global_asm!(include_str!("setjmp.s"));
global_asm!(include_str!("longjmp.s"));

/// Kernel header to announce machine features
#[cfg_attr(target_os = "none", link_section = ".data")]
static mut RAW_BOOT_INFO: Option<&'static RawBootInfo> = None;
static mut BOOT_INFO: Option<BootInfo> = None;

pub fn boot_info() -> &'static BootInfo {
	unsafe { BOOT_INFO.as_ref().unwrap() }
}

#[cfg(feature = "smp")]
pub fn raw_boot_info() -> &'static RawBootInfo {
	unsafe { RAW_BOOT_INFO.unwrap() }
}

/// Serial port to print kernel messages
pub(crate) static COM1: InterruptSpinMutex<Option<SerialPort>> = InterruptSpinMutex::new(None);

pub fn get_ram_address() -> PhysAddr {
	PhysAddr(boot_info().hardware_info.phys_addr_range.start)
}

pub fn get_base_address() -> VirtAddr {
	VirtAddr(boot_info().load_info.kernel_image_addr_range.start)
}

pub fn get_image_size() -> usize {
	let range = &boot_info().load_info.kernel_image_addr_range;
	(range.end - range.start) as usize
}

pub fn get_limit() -> usize {
	boot_info().hardware_info.phys_addr_range.end as usize
}

pub fn get_mbinfo() -> Option<NonZeroU64> {
	match boot_info().platform_info {
		PlatformInfo::Multiboot {
			multiboot_info_addr,
			..
		} => Some(multiboot_info_addr),
		_ => None,
	}
}

pub fn get_fdt() -> Option<Fdt<'static>> {
	boot_info().hardware_info.device_tree.map(|fdt| {
		let ptr = ptr::with_exposed_provenance(fdt.get().try_into().unwrap());
		unsafe { Fdt::from_ptr(ptr).unwrap() }
	})
}

#[cfg(feature = "smp")]
pub fn get_possible_cpus() -> u32 {
	use core::cmp;

	match boot_info().platform_info {
		// FIXME: Remove get_processor_count after a transition period for uhyve 0.1.3 adoption
		PlatformInfo::Uhyve { num_cpus, .. } => cmp::max(
			u32::try_from(num_cpus.get()).unwrap(),
			get_processor_count(),
		),
		_ => apic::local_apic_id_count(),
	}
}

#[cfg(feature = "smp")]
pub fn get_processor_count() -> u32 {
	CPU_ONLINE.load(Ordering::Acquire)
}

#[cfg(not(feature = "smp"))]
pub fn get_processor_count() -> u32 {
	1
}

pub fn is_uhyve_with_pci() -> bool {
	matches!(
		boot_info().platform_info,
		PlatformInfo::Uhyve { has_pci: true, .. }
	)
}

pub fn args() -> Option<&'static str> {
	match boot_info().platform_info {
		PlatformInfo::Multiboot { command_line, .. } => command_line,
		PlatformInfo::LinuxBootParams { command_line, .. } => command_line,
		_ => None,
	}
}

// We can only initialize the serial port here, because VGA requires processor
// configuration first.
/// Earliest initialization function called by the Boot Processor.
pub fn message_output_init() {
	CoreLocal::install();

	let base = boot_info().hardware_info.serial_port_base.unwrap().get();
	let serial_port = unsafe { SerialPort::new(base) };
	*COM1.lock() = Some(serial_port);
}

pub fn output_message_buf(buf: &[u8]) {
	// Output messages to the serial port and VGA screen in unikernel mode.
	COM1.lock().as_mut().unwrap().send(buf);

	#[cfg(feature = "vga")]
	for &byte in buf {
		// vga::write_byte() checks if VGA support has been initialized,
		// so we don't need any additional if clause around it.
		vga::write_byte(byte);
	}
}

/// Real Boot Processor initialization as soon as we have put the first Welcome message on the screen.
#[cfg(target_os = "none")]
pub fn boot_processor_init() {
	processor::detect_features();
	processor::configure();

	if cfg!(feature = "vga") && !env::is_uhyve() {
		#[cfg(feature = "vga")]
		vga::init();
	}

	crate::mm::init();
	crate::mm::print_information();
	CoreLocal::get().add_irq_counter();
	env::init();
	gdt::add_current_core();
	interrupts::load_idt();
	pic::init();

	processor::detect_frequency();
	processor::print_information();
	unsafe {
		trace!("Cr0: {:#x}, Cr4: {:#x}", cr0(), cr4());
	}
	interrupts::install();
	systemtime::init();

	if is_uhyve_with_pci() || !is_uhyve() {
		#[cfg(feature = "pci")]
		pci::init();
	}
	if !env::is_uhyve() {
		#[cfg(feature = "acpi")]
		acpi::init();
	}

	apic::init();
	scheduler::install_timer_handler();
	serial::install_serial_interrupt();
	finish_processor_init();
	interrupts::enable();
}

/// Boots all available Application Processors on bare-metal or QEMU.
/// Called after the Boot Processor has been fully initialized along with its scheduler.
#[cfg(target_os = "none")]
pub fn boot_application_processors() {
	#[cfg(feature = "smp")]
	apic::boot_application_processors();
	apic::print_information();
}

/// Application Processor initialization
#[cfg(all(target_os = "none", feature = "smp"))]
pub fn application_processor_init() {
	CoreLocal::install();
	processor::configure();
	gdt::add_current_core();
	interrupts::load_idt();
	apic::init_x2apic();
	apic::init_local_apic();
	unsafe {
		trace!("Cr0: {:#x}, Cr4: {:#x}", cr0(), cr4());
	}
	interrupts::enable();
	finish_processor_init();
}

fn finish_processor_init() {
	if env::is_uhyve() {
		// uhyve does not use apic::detect_from_acpi and therefore does not know the number of processors and
		// their APIC IDs in advance.
		// Therefore, we have to add each booted processor into the CPU_LOCAL_APIC_IDS vector ourselves.
		// Fortunately, the Local APIC IDs of uhyve are sequential and therefore match the Core IDs.
		apic::add_local_apic_id(core_id() as u8);

		// uhyve also boots each processor into _start itself and does not use apic::boot_application_processors.
		// Therefore, the current processor already needs to prepare the processor variables for a possible next processor.
		apic::init_next_processor_variables();
	}

	// This triggers apic::boot_application_processors (bare-metal/QEMU) or uhyve
	// to initialize the next processor.
	CPU_ONLINE.fetch_add(1, Ordering::Release);
}

pub fn print_statistics() {
	interrupts::print_statistics();
}

/// `CPU_ONLINE` is the count of CPUs that finished initialization.
///
/// It also synchronizes initialization of CPU cores.
pub static CPU_ONLINE: AtomicU32 = AtomicU32::new(0);

pub static CURRENT_STACK_ADDRESS: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());

#[cfg(target_os = "none")]
#[inline(never)]
#[no_mangle]
unsafe extern "C" fn pre_init(boot_info: &'static RawBootInfo, cpu_id: u32) -> ! {
	// Enable caching
	unsafe {
		let mut cr0 = cr0();
		cr0.remove(Cr0::CR0_CACHE_DISABLE | Cr0::CR0_NOT_WRITE_THROUGH);
		cr0_write(cr0);
	}

	unsafe {
		RAW_BOOT_INFO = Some(boot_info);
		BOOT_INFO = Some(BootInfo::from(*boot_info));
	}

	if cpu_id == 0 {
		crate::boot_processor_main()
	} else {
		#[cfg(not(feature = "smp"))]
		{
			error!("SMP support deactivated");
			loop {
				processor::halt();
			}
		}
		#[cfg(feature = "smp")]
		crate::application_processor_main();
	}
}

#[cfg(feature = "common-os")]
const LOADER_START: usize = 0x10000000000;
#[cfg(feature = "common-os")]
const LOADER_STACK_SIZE: usize = 0x8000;

#[cfg(feature = "common-os")]
pub fn load_application<F>(code_size: u64, tls_size: u64, func: F) -> Result<(), ()>
where
	F: FnOnce(&'static mut [u8], Option<&'static mut [u8]>) -> Result<(), ()>,
{
	use core::ptr::slice_from_raw_parts_mut;

	use align_address::Align;
	use x86_64::structures::paging::{PageSize, Size4KiB as BasePageSize};

	use crate::arch::x86_64::mm::paging::{self, PageTableEntryFlags, PageTableEntryFlagsExt};
	use crate::arch::x86_64::mm::physicalmem;

	let code_size = (code_size as usize + LOADER_STACK_SIZE).align_up(BasePageSize::SIZE as usize);
	let physaddr =
		physicalmem::allocate_aligned(code_size as usize, BasePageSize::SIZE as usize).unwrap();

	let mut flags = PageTableEntryFlags::empty();
	flags.normal().writable().user().execute_enable();
	paging::map::<BasePageSize>(
		VirtAddr::from(LOADER_START),
		physaddr,
		code_size / BasePageSize::SIZE as usize,
		flags,
	);

	let code_slice = unsafe { &mut *slice_from_raw_parts_mut(LOADER_START as *mut u8, code_size) };

	if tls_size > 0 {
		// To access TLS blocks on x86-64, TLS offsets are *subtracted* from the thread register value.
		// So the thread pointer needs to be `block_ptr + tls_offset`.
		// GNU style TLS requires `fs:0` to represent the same address as the thread pointer.
		// Since the thread pointer points to the end of the TLS blocks, we need to store it there.
		let tcb_size = core::mem::size_of::<*mut ()>();
		let tls_offset = tls_size as usize;

		let tls_memsz = (tls_offset + tcb_size).align_up(BasePageSize::SIZE as usize);
		let physaddr =
			physicalmem::allocate_aligned(tls_memsz, BasePageSize::SIZE as usize).unwrap();

		let mut flags = PageTableEntryFlags::empty();
		flags.normal().writable().user().execute_disable();
		let tls_virt = VirtAddr::from(LOADER_START + code_size + BasePageSize::SIZE as usize);
		paging::map::<BasePageSize>(
			tls_virt,
			physaddr,
			tls_memsz / BasePageSize::SIZE as usize,
			flags,
		);
		let block = unsafe {
			&mut *slice_from_raw_parts_mut(tls_virt.as_mut_ptr() as *mut u8, tls_offset + tcb_size)
		};
		for elem in block.iter_mut() {
			*elem = 0;
		}

		// thread_ptr = block_ptr + tls_offset
		let thread_ptr = block[tls_offset..].as_mut_ptr().cast::<()>();
		unsafe {
			thread_ptr.cast::<*mut ()>().write(thread_ptr);
		}
		crate::arch::x86_64::kernel::processor::writefs(thread_ptr as usize);

		func(code_slice, Some(block))
	} else {
		func(code_slice, None)
	}
}

#[cfg(feature = "common-os")]
pub unsafe fn jump_to_user_land(entry_point: u64, code_size: u64) -> ! {
	use align_address::Align;
	use x86_64::structures::paging::{PageSize, Size4KiB as BasePageSize};

	use crate::arch::x86_64::kernel::scheduler::TaskStacks;
	use crate::executor::block_on;

	info!("Create new file descriptor table");
	block_on(core_scheduler().recreate_objmap(), None).unwrap();

	let ds = 0x23u64;
	let cs = 0x2bu64;
	let entry_point: u64 = (LOADER_START as u64) | entry_point;
	let stack_pointer: u64 = LOADER_START as u64
		+ (code_size + LOADER_STACK_SIZE as u64).align_up(BasePageSize::SIZE)
		- 128 /* red zone */ - 8;

	debug!(
		"Jump to user space at 0x{:x}, stack pointer 0x{:x}",
		entry_point, stack_pointer
	);
	unsafe {
		asm!(
			"and rsp, {0}",
			"swapgs",
			"push {1}",
			"push {2}",
			"push {3}",
			"push {4}",
			"push {5}",
			"iretq",
			const u64::MAX - (TaskStacks::MARKER_SIZE as u64 - 1),
			in(reg) ds,
			in(reg) stack_pointer,
			const 0x1202u64,
			in(reg) cs,
			in(reg) entry_point,
			options(nostack, noreturn)
		);
	}
}
