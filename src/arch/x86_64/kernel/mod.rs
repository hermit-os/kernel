#[cfg(feature = "common-os")]
use core::arch::asm;
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering};
use core::task::Waker;

use hermit_entry::boot_info::{PlatformInfo, RawBootInfo};
use memory_addresses::{PhysAddr, VirtAddr};
use x86_64::registers::control::{Cr0, Cr4};

use self::serial::SerialPort;
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

pub(crate) struct Console {
	serial_port: SerialPort,
}

impl Console {
	pub fn new() -> Self {
		CoreLocal::install();

		let base = env::boot_info()
			.hardware_info
			.serial_port_base
			.unwrap()
			.get();
		let serial_port = unsafe { SerialPort::new(base) };
		Self { serial_port }
	}

	pub fn write(&mut self, buf: &[u8]) {
		self.serial_port.send(buf);

		#[cfg(feature = "vga")]
		for &byte in buf {
			// vga::write_byte() checks if VGA support has been initialized,
			// so we don't need any additional if clause around it.
			vga::write_byte(byte);
		}
	}

	pub fn buffer_input(&mut self) {
		self.serial_port.buffer_input();
	}

	pub fn read(&mut self) -> Option<u8> {
		self.serial_port.read()
	}

	pub fn is_empty(&self) -> bool {
		self.serial_port.is_empty()
	}

	pub fn register_waker(&mut self, waker: &Waker) {
		self.serial_port.register_waker(waker);
	}
}

impl Default for Console {
	fn default() -> Self {
		Self::new()
	}
}

pub fn get_ram_address() -> PhysAddr {
	PhysAddr::new(env::boot_info().hardware_info.phys_addr_range.start)
}

pub fn get_base_address() -> VirtAddr {
	VirtAddr::new(env::boot_info().load_info.kernel_image_addr_range.start)
}

pub fn get_image_size() -> usize {
	let range = &env::boot_info().load_info.kernel_image_addr_range;
	(range.end - range.start) as usize
}

#[cfg(feature = "smp")]
pub fn get_possible_cpus() -> u32 {
	use core::cmp;

	match env::boot_info().platform_info {
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
		env::boot_info().platform_info,
		PlatformInfo::Uhyve { has_pci: true, .. }
	)
}

pub fn args() -> Option<&'static str> {
	match env::boot_info().platform_info {
		PlatformInfo::Multiboot { command_line, .. } => command_line,
		PlatformInfo::LinuxBootParams { command_line, .. } => command_line,
		_ => None,
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
	crate::logging::KERNEL_LOGGER.set_time(true);
	gdt::add_current_core();
	interrupts::load_idt();
	pic::init();

	processor::detect_frequency();
	processor::print_information();
	debug!("Cr0 = {:?}", Cr0::read());
	debug!("Cr4 = {:?}", Cr4::read());
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
	debug!("Cr0 = {:?}", Cr0::read());
	debug!("Cr4 = {:?}", Cr4::read());
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
}

pub fn boot_next_processor() {
	// This triggers apic::boot_application_processors (bare-metal/QEMU) or uhyve
	// to initialize the next processor.
	let cpu_online = CPU_ONLINE.fetch_add(1, Ordering::Release);

	if !env::is_uhyve() {
		if cpu_online == 0 {
			#[cfg(all(target_os = "none", feature = "smp"))]
			apic::boot_application_processors();
		}

		if !cfg!(feature = "smp") {
			apic::print_information();
		}
	}
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
#[unsafe(no_mangle)]
unsafe extern "C" fn pre_init(boot_info: Option<&'static RawBootInfo>, cpu_id: u32) -> ! {
	use x86_64::registers::control::Cr0Flags;

	// Enable caching
	unsafe {
		Cr0::update(|flags| flags.remove(Cr0Flags::CACHE_DISABLE | Cr0Flags::NOT_WRITE_THROUGH));
	}

	if cpu_id == 0 {
		env::set_boot_info(*boot_info.unwrap());

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
const LOADER_START: usize = 0x0100_0000_0000;
#[cfg(feature = "common-os")]
const LOADER_STACK_SIZE: usize = 0x8000;

#[cfg(feature = "common-os")]
pub fn load_application<F, T>(code_size: u64, tls_size: u64, func: F) -> T
where
	F: FnOnce(&'static mut [u8], Option<&'static mut [u8]>) -> T,
{
	use core::ptr::slice_from_raw_parts_mut;

	use align_address::Align;
	use x86_64::structures::paging::{PageSize, Size4KiB as BasePageSize};

	use crate::arch::x86_64::mm::paging::{self, PageTableEntryFlags, PageTableEntryFlagsExt};
	use crate::mm::physicalmem;

	let code_size = (code_size as usize + LOADER_STACK_SIZE).align_up(BasePageSize::SIZE as usize);
	let physaddr = physicalmem::allocate_aligned(code_size, BasePageSize::SIZE as usize).unwrap();

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
		let block =
			unsafe { &mut *slice_from_raw_parts_mut(tls_virt.as_mut_ptr(), tls_offset + tcb_size) };
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
pub unsafe fn jump_to_user_land(entry_point: usize, code_size: usize, arg: &[&str]) -> ! {
	use alloc::ffi::CString;

	use align_address::Align;
	use x86_64::structures::paging::{PageSize, Size4KiB as BasePageSize};

	use crate::arch::x86_64::kernel::scheduler::TaskStacks;
	use crate::executor::block_on;

	info!("Create new file descriptor table");
	block_on(core_scheduler().recreate_objmap(), None).unwrap();

	let entry_point: usize = LOADER_START | entry_point;
	let stack_pointer: usize = LOADER_START
		+ (code_size + LOADER_STACK_SIZE).align_up(BasePageSize::SIZE.try_into().unwrap())
		- 8;

	let stack_pointer =
		stack_pointer - 128 /* red zone */ - arg.len() * core::mem::size_of::<*mut u8>();
	let argv = unsafe { core::slice::from_raw_parts_mut(stack_pointer as *mut *mut u8, arg.len()) };
	let len = arg.iter().fold(0, |acc, x| acc + x.len() + 1);
	// align stack pointer to fulfill the requirements of the x86_64 ABI
	let stack_pointer = (stack_pointer - len).align_down(16) - core::mem::size_of::<usize>();

	let mut pos: usize = 0;
	for (i, s) in arg.iter().enumerate() {
		if let Ok(s) = CString::new(*s) {
			let bytes = s.as_bytes_with_nul();
			argv[i] = (stack_pointer + pos) as *mut u8;
			pos += bytes.len();

			unsafe {
				core::ptr::copy_nonoverlapping(bytes.as_ptr(), argv[i], bytes.len());
			}
		} else {
			panic!("Unable to create C string!");
		}
	}

	debug!("Jump to user space at 0x{entry_point:x}, stack pointer 0x{stack_pointer:x}");

	unsafe {
		asm!(
			"and rsp, {0}",
			"swapgs",
			"push {1}",
			"push {2}",
			"push {3}",
			"push {4}",
			"push {5}",
			"mov rdi, {6}",
			"mov rsi, {7}",
			"iretq",
			const u64::MAX - (TaskStacks::MARKER_SIZE as u64 - 1),
			const 0x23usize,
			in(reg) stack_pointer,
			const 0x1202u64,
			const 0x2busize,
			in(reg) entry_point,
			in(reg) argv.len(),
			in(reg) argv.as_ptr(),
			options(nostack, noreturn)
		);
	}
}
