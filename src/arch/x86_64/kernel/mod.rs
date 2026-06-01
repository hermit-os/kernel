#[cfg(feature = "common-os")]
use core::arch::asm;
use core::ptr;
#[cfg(feature = "common-os")]
use core::slice;
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use hermit_entry::boot_info::RawBootInfo;
use x86_64::registers::control::{Cr0, Cr4};

pub(crate) use self::apic::{set_oneshot_timer, wakeup_core};
use crate::arch::x86_64::kernel::core_local::*;
use crate::env;

#[cfg(feature = "acpi")]
pub mod acpi;
pub mod apic;
pub mod core_local;
pub mod gdt;
pub mod interrupts;
#[cfg(feature = "kernel-stack")]
pub mod kernel_stack;
#[cfg(all(not(feature = "pci"), feature = "virtio"))]
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
pub mod vga;

#[cfg(feature = "smp")]
pub fn get_possible_cpus() -> u32 {
	#[cfg(feature = "uhyve")]
	if let Some(num_cpus) = env::uhyve_num_cpus() {
		return num_cpus.get().try_into().unwrap();
	}

	apic::local_apic_id_count()
}

#[cfg(feature = "smp")]
pub fn get_processor_count() -> u32 {
	CPU_ONLINE.load(Ordering::Acquire)
}

#[cfg(not(feature = "smp"))]
pub fn get_processor_count() -> u32 {
	1
}

/// Real Boot Processor initialization as soon as we have put the first Welcome message on the screen.
#[cfg(target_os = "none")]
pub fn boot_processor_init() {
	processor::detect_features();
	processor::configure();

	#[cfg(feature = "vga")]
	vga::init();

	crate::mm::init();
	crate::mm::print_information();
	CoreLocal::get().add_irq_counter();
	gdt::add_current_core();
	interrupts::load_idt();
	pic::init();

	processor::detect_frequency();
	crate::logging::KERNEL_LOGGER.set_time(true);
	processor::print_information();
	debug!("Cr0 = {:?}", Cr0::read());
	debug!("Cr4 = {:?}", Cr4::read());
	interrupts::install();
	systemtime::init();

	#[cfg(feature = "acpi")]
	acpi::init();

	#[cfg(feature = "pci")]
	pci::init();

	apic::init();
	scheduler::install_timer_handler();
	finish_processor_init();
}

/// Application Processor initialization
#[cfg(all(target_os = "none", feature = "smp"))]
pub fn application_processor_init() {
	CoreLocal::install();
	processor::configure();
	gdt::add_current_core();
	interrupts::load_idt();
	if processor::supports_x2apic() {
		apic::init_x2apic();
	}
	apic::init_local_apic();
	debug!("Cr0 = {:?}", Cr0::read());
	debug!("Cr4 = {:?}", Cr4::read());
	finish_processor_init();
}

fn finish_processor_init() {
	#[cfg(feature = "uhyve")]
	if env::is_uhyve() {
		// uhyve does not use apic::detect_from_acpi and therefore does not know the number of processors and
		// their APIC IDs in advance.
		// Therefore, we have to add each booted processor into the CPU_LOCAL_APIC_IDS vector ourselves.
		// Fortunately, the Local APIC IDs of uhyve are sequential and therefore match the Core IDs.
		apic::add_local_apic_id(core_id() as u8);

		// uhyve also boots each processor into _start itself and does not use apic::boot_application_processors.
		// Therefore, the current processor already needs to prepare the processor variables for a possible next processor.
		#[cfg(feature = "smp")]
		apic::init_next_processor_variables();
	}
}

pub fn boot_next_processor() {
	// This triggers apic::boot_application_processors (bare-metal/QEMU) or uhyve
	// to initialize the next processor.
	let cpu_online = CPU_ONLINE.fetch_add(1, Ordering::Release);

	#[cfg(feature = "uhyve")]
	if env::is_uhyve() {
		return;
	}

	if cpu_online == 0 {
		#[cfg(all(target_os = "none", feature = "smp"))]
		apic::boot_application_processors();
	}

	if !cfg!(feature = "smp") {
		apic::print_information();
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
			let style = anstyle::Style::new().fg_color(Some(anstyle::AnsiColor::Red.into()));
			let preamble = format_args!("[            ][{cpu_id}][{style}ERROR{style:#}]");
			println!(
				"{preamble} Secondary core booted, but Hermit was not built with SMP support!"
			);
			loop {
				processor::halt();
			}
		}
		#[cfg(feature = "smp")]
		crate::application_processor_main();
	}
}

#[cfg(feature = "common-os")]
pub(crate) const USER_START: VirtAddr = VirtAddr::new(0x0100_0000_0000);
#[cfg(feature = "common-os")]
const USER_STACK: VirtAddr = VirtAddr::new(0x0180_0000_0000 - USER_STACK_SIZE as u64);
#[cfg(feature = "common-os")]
const USER_STACK_SIZE: usize = 0x8000;

#[cfg(feature = "common-os")]
pub fn load_application<F>(code_size: u64, tls_size: u64, func: F) -> Result<(), ()>
where
	F: FnOnce(
		&'static mut [u8],
		Option<&'static mut [u8]>,
	) -> Result<Option<alloc::vec::Vec<u8>>, ()>,
{
	use align_address::Align;
	use free_list::PageLayout;
	use memory_addresses::{PhysAddr, VirtAddr};
	use x86_64::structures::paging::{PageSize, Size4KiB as BasePageSize};

	use crate::arch::x86_64::mm::paging::{self, PageTableEntryFlags, PageTableEntryFlagsExt};
	use crate::mm::{FrameAlloc, PageRangeAllocator};
	use crate::fd::{Fd, RawFd};
	use crate::fd::stdio;
	#[cfg(feature = "fork")]
	use crate::mm::frame_ref_inc;
	use crate::mm::vma::*;

	// each process has to provide its own object_map
	// => create a new one
	let mut object_map = HashMap::<
			RawFd,
			Arc<async_lock::RwLock<Fd>>,
			RandomState,
		>::with_hasher(
			RandomState::with_seeds(0, 0, 0, 0),
		);
	stdio::setup(&mut object_map);
	core_scheduler().set_current_task_object_map(Arc::new(RwSpinLock::new(object_map)));

	let code_size = (code_size as usize).align_up(BasePageSize::SIZE as usize);
	let layout = PageLayout::from_size_align(code_size, BasePageSize::SIZE as usize).unwrap();
	let frame_range = FrameAlloc::allocate(layout).unwrap();
	let physaddr = PhysAddr::from(frame_range.start());
	#[cfg(feature = "fork")]
	for i in 0..code_size / BasePageSize::SIZE as usize {
		frame_ref_inc(physaddr + i * BasePageSize::SIZE as usize);
	}

	let mut flags = PageTableEntryFlags::empty();
	flags.normal().writable().user().execute_enable();
	paging::map::<BasePageSize>(
		VirtAddr::from(USER_START),
		physaddr,
		code_size / BasePageSize::SIZE as usize,
		flags,
	);
	// VirtAddr's defined in vma.rs is `x86_64::VirtAddr` on x86_64 but
	// this file's local `use` brings in `memory_addresses::VirtAddr`.
	// Convert at the boundary so the BTreeMap-keyed insert type-checks.
	{
		let start: x86_64::VirtAddr = VirtAddr::from(USER_START).into();
		let end: x86_64::VirtAddr =
			VirtAddr::from(USER_START + code_size).align_up(BasePageSize::SIZE).into();
		core_scheduler().get_current_task().borrow_mut().vmas.write().insert(
			start,
			VirtualMemoryArea::new(
				start,
				end,
				VirtualMemoryAreaProt::READ | VirtualMemoryAreaProt::WRITE | VirtualMemoryAreaProt::EXECUTE,
				MemoryType::CODE,
			),
		);
	}

	let loader_start_ptr = ptr::with_exposed_provenance_mut(USER_START.as_usize());
	let code_slice = unsafe { slice::from_raw_parts_mut(loader_start_ptr, code_size) };

	if tls_size > 0 {
		// To access TLS blocks on x86-64, TLS offsets are *subtracted* from the thread register value.
		// So the thread pointer needs to be `block_ptr + tls_offset`.
		// GNU style TLS requires `fs:0` to represent the same address as the thread pointer.
		// Since the thread pointer points to the end of the TLS blocks, we need to store it there.
		let tcb_size = size_of::<*mut ()>();
		let tls_offset = tls_size as usize;

		let tls_memsz = (tls_offset + tcb_size).align_up(BasePageSize::SIZE as usize);
		let layout = PageLayout::from_size(tls_memsz).unwrap();
		let frame_range = FrameAlloc::allocate(layout).unwrap();
		let physaddr = PhysAddr::from(frame_range.start());
		#[cfg(feature = "fork")]
		for i in 0..tls_memsz / BasePageSize::SIZE as usize {
			frame_ref_inc(physaddr + i * BasePageSize::SIZE as usize);
		}

		let mut flags = PageTableEntryFlags::empty();
		flags.normal().writable().user().execute_disable();
		let tls_virt = VirtAddr::from(USER_START.as_usize() + code_size + BasePageSize::SIZE as usize);
		paging::map::<BasePageSize>(
			tls_virt,
			physaddr,
			tls_memsz / BasePageSize::SIZE as usize,
			flags,
		);
	
		{
			let start: x86_64::VirtAddr = tls_virt.into();
			let end: x86_64::VirtAddr =
				(tls_virt + tls_memsz).align_up(BasePageSize::SIZE).into();
			core_scheduler().get_current_task().borrow_mut().vmas.write().insert(
				start,
				VirtualMemoryArea::new(
					start,
					end,
					VirtualMemoryAreaProt::READ | VirtualMemoryAreaProt::WRITE,
					MemoryType::TLS
				),
			);
		}
	
		let block =
			unsafe { slice::from_raw_parts_mut(tls_virt.as_mut_ptr(), tls_offset + tcb_size) };
		for elem in block.iter_mut() {
			*elem = 0;
		}

		// thread_ptr = block_ptr + tls_offset
		let thread_ptr = block[tls_offset..].as_mut_ptr().cast::<()>();
		unsafe {
			thread_ptr.cast::<*mut ()>().write(thread_ptr);
		}
		processor::writefs(thread_ptr.expose_provenance());

		// Run the ELF loader, which copies the binary's `PT_TLS` initial
		// image into `block` and returns the pristine PT_TLS image
		// directly from the ELF buffer.
		let tls_init = func(code_slice, Some(block))?;

		if let Some(init) = tls_init {
			let template = Arc::new(
				crate::scheduler::task::TlsTemplate {
					size: tls_offset,
					init,
				},
			);
			core_scheduler()
				.get_current_task()
				.borrow_mut()
				.tls_template = Some(template);
		}

		Ok(())
	} else {
		func(code_slice, None)?;
		Ok(())
	}
}

#[cfg(feature = "common-os")]
pub unsafe fn jump_to_user_land(entry_point: usize, arg: alloc::vec::Vec<&str>) -> ! {
	use alloc::ffi::CString;

	use align_address::Align;
	use free_list::PageLayout;
	use x86_64::structures::paging::{PageSize, Size4KiB as BasePageSize};
	use x86_64::structures::paging::PageTableFlags as PageTableEntryFlags;

	use crate::arch::x86_64::mm::paging::PageTableEntryFlagsExt;
	use crate::arch::x86_64::kernel::scheduler::TaskStacks;
	use crate::mm::{FrameAlloc, PageRangeAllocator};
	use crate::arch::mm::paging;
	#[cfg(feature = "fork")]
	use crate::mm::frame_ref_inc;
	use crate::mm::vma::*;

	debug!("Create new file descriptor table");
	core_scheduler().recreate_objmap().unwrap();

	let entry_point: usize = USER_START.as_usize() | entry_point;
	let stack_pointer: usize = USER_STACK.as_usize() + USER_STACK_SIZE - 8;

	let layout = PageLayout::from_size(USER_STACK_SIZE).unwrap();
	let frame_range = FrameAlloc::allocate(layout).unwrap();
	let phys_addr = PhysAddr::from(frame_range.start());
	let mut flags = PageTableEntryFlags::empty();
	flags.normal().writable().user();
	paging::map::<BasePageSize>(
		USER_STACK,
		phys_addr,
		USER_STACK_SIZE / BasePageSize::SIZE as usize,
		flags,
	);
	{
		let start: x86_64::VirtAddr = USER_STACK.into();
		let end: x86_64::VirtAddr = (USER_STACK+USER_STACK_SIZE).into();
		core_scheduler().get_current_task().borrow_mut().vmas.write().insert(start, VirtualMemoryArea::new(start, end, VirtualMemoryAreaProt::READ|VirtualMemoryAreaProt::WRITE, MemoryType::STACK));
	}
	#[cfg(feature = "fork")]
	for i in 0..USER_STACK_SIZE / BasePageSize::SIZE as usize {
		frame_ref_inc(phys_addr + i * BasePageSize::SIZE as usize);
	}

	let stack_pointer = stack_pointer - 128 /* red zone */ - arg.len() * size_of::<*mut u8>();
	let stack_ptr = ptr::with_exposed_provenance_mut::<*mut u8>(stack_pointer);
	let argv = unsafe { slice::from_raw_parts_mut(stack_ptr, arg.len()) };
	let len = arg.iter().fold(0, |acc, x| acc + x.len() + 1);
	// align stack pointer to fulfill the requirements of the x86_64 ABI
	let stack_pointer = (stack_pointer - len).align_down(16) - size_of::<usize>();

	let mut pos: usize = 0;
	for (i, s) in arg.iter().enumerate() {
		let s = CString::new(*s).unwrap();
		let bytes = s.as_bytes_with_nul();
		argv[i] = ptr::with_exposed_provenance_mut::<u8>(stack_pointer + pos);
		pos += bytes.len();

		unsafe {
			argv[i].copy_from_nonoverlapping(bytes.as_ptr(), bytes.len());
		}
	}

	drop(arg);

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
			// Clear registers so that state cannot leak.
			"xor rax, rax", "xor rbx, rbx", "xor rcx, rcx", "xor rdx, rdx",
			"xor r8, r8", "xor r9, r9", "xor r10, r10",
			"xor r11, r11", "xor r12, r12", "xor r13, r13",
			"xor r14, r14", "xor r15, r15",
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
