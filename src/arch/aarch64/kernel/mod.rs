pub mod core_local;
pub mod interrupts;
#[cfg(feature = "kernel-stack")]
pub mod kernel_stack;
mod lscpu;
#[cfg(all(not(feature = "pci"), feature = "virtio"))]
pub mod mmio;
#[cfg(feature = "pci")]
pub mod pci;
pub mod processor;
pub mod scheduler;
pub mod serial;
#[cfg(target_os = "none")]
mod start;
pub mod systemtime;

use alloc::alloc::alloc;
use core::alloc::Layout;
use core::arch::global_asm;
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

pub(crate) use self::interrupts::wakeup_core;
pub(crate) use self::processor::set_oneshot_timer;
use crate::arch::aarch64::kernel::core_local::*;
use crate::arch::aarch64::mm::paging::{BasePageSize, PageSize};
use crate::config::*;

#[repr(align(8))]
pub(crate) struct AlignedAtomicU32(AtomicU32);

/// `CPU_ONLINE` is the count of CPUs that finished initialization.
///
/// It also synchronizes initialization of CPU cores.
pub(crate) static CPU_ONLINE: AlignedAtomicU32 = AlignedAtomicU32(AtomicU32::new(0));

pub(crate) static CURRENT_STACK_ADDRESS: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());

#[cfg(target_os = "none")]
global_asm!(include_str!("start.s"));

#[cfg(feature = "smp")]
pub fn get_possible_cpus() -> u32 {
	let fdt = crate::env::fdt().unwrap();
	let cpu_count = fdt.cpus().count();
	u32::try_from(cpu_count).unwrap()
}

#[cfg(feature = "smp")]
pub fn get_processor_count() -> u32 {
	CPU_ONLINE.0.load(Ordering::Acquire)
}

#[cfg(not(feature = "smp"))]
pub fn get_processor_count() -> u32 {
	1
}

/// Real Boot Processor initialization as soon as we have put the first Welcome message on the screen.
#[cfg(target_os = "none")]
pub fn boot_processor_init() {
	processor::configure();

	crate::mm::init();
	crate::mm::print_information();
	CoreLocal::get().add_irq_counter();
	interrupts::init();
	processor::detect_frequency();
	crate::logging::KERNEL_LOGGER.set_time(true);
	processor::print_information();
	systemtime::init();
	#[cfg(feature = "pci")]
	pci::init();

	finish_processor_init();
}

/// Application Processor initialization
#[allow(dead_code)]
pub fn application_processor_init() {
	CoreLocal::install();
	interrupts::init_cpu();
	finish_processor_init();
}

fn finish_processor_init() {
	debug!("Initialized processor {}", core_id());

	// Allocate stack for the CPU and pass the addresses.
	let layout = Layout::from_size_align(KERNEL_STACK_SIZE, BasePageSize::SIZE as usize).unwrap();
	let stack = unsafe { alloc(layout) };
	assert!(!stack.is_null());
	CURRENT_STACK_ADDRESS.store(stack, Ordering::Relaxed);
}

pub fn boot_next_processor() {
	// This triggers to wake up the next processor (bare-metal/QEMU) or uhyve
	// to initialize the next processor.
	#[allow(unused_variables)]
	let cpu_online = CPU_ONLINE.0.fetch_add(1, Ordering::Release);

	#[allow(clippy::needless_return)]
	#[cfg(feature = "uhyve")]
	if crate::env::is_uhyve() {
		return;
	}

	#[cfg(all(target_os = "none", feature = "smp"))]
	if get_possible_cpus() > 1 {
		use core::arch::asm;
		use core::hint::spin_loop;

		use memory_addresses::VirtAddr;

		use crate::arch::aarch64::kernel::start::{TTBR0, smp_start};
		use crate::mm::virtual_to_physical;

		if cpu_online == 0 {
			use aarch64_cpu::registers::{Readable, TTBR0_EL1};

			let virt_start = VirtAddr::from_ptr(smp_start as *const ());
			let phys_start = virtual_to_physical(virt_start).unwrap();
			assert!(virt_start.as_u64() == phys_start.as_u64());

			trace!("Virtual address of smp_start 0x{virt_start:x}");
			trace!("Physical address of smp_start 0x{phys_start:x}");

			let fdt = crate::env::fdt().unwrap();
			let psci_node = fdt.find_node("/psci").unwrap();

			let cpu_on = psci_node.property("cpu_on").unwrap().as_usize().unwrap();
			let cpu_on = u32::try_from(cpu_on).unwrap();
			trace!("CPU_ON: 0x{cpu_on:x}");

			let method = psci_node
				.property("method")
				.map(|node| node.as_str().unwrap())
				.unwrap_or("unknown");

			let ttbr0_addr = TTBR0_EL1.get();
			let ttbr0_ptr = ptr::with_exposed_provenance_mut(ttbr0_addr.try_into().unwrap());
			TTBR0.store(ttbr0_ptr, Ordering::Relaxed);

			for cpu_id in 1..get_possible_cpus() {
				debug!("Try to wake-up core {cpu_id}");

				if method == "hvc" {
					// call hypervisor to wakeup next core
					unsafe {
						asm!("hvc #0", in("x0") cpu_on, in("x1") cpu_id, in("x2") phys_start.as_u64(), in("x3") cpu_id, options(nomem, nostack));
					}
				} else if method == "smc" {
					// call secure monitor to wakeup next core
					unsafe {
						asm!("smc #0", in("x0") cpu_on, in("x1") cpu_id, in("x2") phys_start.as_u64(), in("x3") cpu_id, options(nomem, nostack));
					}
				} else {
					warn!("Method {method} isn't supported!");
					return;
				}

				// wait for next core
				while CPU_ONLINE.0.load(Ordering::Relaxed) < cpu_id + 1 {
					spin_loop();
				}
			}
		}
	}
}

pub fn print_statistics() {
	interrupts::print_statistics();
}

#[cfg(feature = "common-os")]
pub(crate) const LOADER_START: usize = 0x0100_0000_0000;
#[cfg(feature = "common-os")]
const LOADER_STACK_SIZE: usize = 0x8000;

/// Map the user-mode binary into the address space and run the ELF-loader
/// closure against the freshly-mapped pages. Mirrors the x86_64 sibling
/// (`arch::x86_64::kernel::load_application`).
#[cfg(feature = "common-os")]
pub fn load_application<F>(code_size: u64, tls_size: u64, func: F) -> Result<(), ()>
where
	F: FnOnce(
		&'static mut [u8],
		Option<&'static mut [u8]>,
	) -> Result<Option<alloc::vec::Vec<u8>>, ()>,
{
	use alloc::sync::Arc;

	use ahash::RandomState;
	use align_address::Align;
	use free_list::PageLayout;
	use hashbrown::HashMap;
	use hermit_sync::RwSpinLock;

	use crate::arch::aarch64::mm::paging::{self, PageTableEntryFlags};
	use crate::fd::stdio::*;
	use crate::fd::{Fd, RawFd, STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO};
	use crate::mm::{FrameAlloc, PageRangeAllocator};
	#[cfg(feature = "fork")]
	use crate::mm::frame_ref_inc;

	// Each process has its own object map.
	let mut object_map = HashMap::<RawFd, Arc<async_lock::RwLock<Fd>>, RandomState>::with_hasher(
		RandomState::with_seeds(0, 0, 0, 0),
	);
	if env::is_uhyve() {
		let stdin = Arc::new(async_lock::RwLock::new(UhyveStdin::new().into()));
		let stdout = Arc::new(async_lock::RwLock::new(UhyveStdout::new().into()));
		let stderr = Arc::new(async_lock::RwLock::new(UhyveStderr::new().into()));
		object_map.insert(STDIN_FILENO, stdin);
		object_map.insert(STDOUT_FILENO, stdout);
		object_map.insert(STDERR_FILENO, stderr);
	} else {
		let stdin = Arc::new(async_lock::RwLock::new(GenericStdin::new().into()));
		let stdout = Arc::new(async_lock::RwLock::new(GenericStdout::new().into()));
		let stderr = Arc::new(async_lock::RwLock::new(GenericStderr::new().into()));
		object_map.insert(STDIN_FILENO, stdin);
		object_map.insert(STDOUT_FILENO, stdout);
		object_map.insert(STDERR_FILENO, stderr);
	}
	core_scheduler().set_current_task_object_map(Arc::new(RwSpinLock::new(object_map)));

	let code_size = (code_size as usize + LOADER_STACK_SIZE).align_up(BasePageSize::SIZE as usize);
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
		VirtAddr::from(LOADER_START),
		physaddr,
		code_size / BasePageSize::SIZE as usize,
		flags,
	);

	let loader_start_ptr = ptr::with_exposed_provenance_mut(LOADER_START);
	let code_slice = unsafe { slice::from_raw_parts_mut(loader_start_ptr, code_size) };

	if tls_size > 0 {
		// AArch64 uses TLS Variant I: the thread pointer (TPIDR_EL0) points to
		// the TCB; the TLS image follows immediately after a two-word reserved
		// area (`tcb[0] = dtv`, `tcb[1]` reserved). We allocate the TCB plus
		// the TLS image as one contiguous block so a single `msr tpidr_el0`
		// suffices and the layout matches what musl/glibc expect.
		let tcb_size = 2 * mem::size_of::<*mut ()>();
		let tls_offset = tcb_size;

		let tls_memsz = (tls_offset + tls_size as usize).align_up(BasePageSize::SIZE as usize);
		let layout = PageLayout::from_size(tls_memsz).unwrap();
		let frame_range = FrameAlloc::allocate(layout).unwrap();
		let physaddr = PhysAddr::from(frame_range.start());
		#[cfg(feature = "fork")]
		for i in 0..tls_memsz / BasePageSize::SIZE as usize {
			frame_ref_inc(physaddr + i * BasePageSize::SIZE as usize);
		}

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
			unsafe { slice::from_raw_parts_mut(tls_virt.as_mut_ptr(), tls_offset + tls_size as usize) };
		for elem in block.iter_mut() {
			*elem = 0;
		}

		// Variant I: TPIDR_EL0 points at the TCB; user code finds its data at
		// TPIDR_EL0 + tls_offset.
		let thread_ptr = block.as_mut_ptr().cast::<()>();
		set_user_tpidr_el0(thread_ptr.expose_provenance() as u64);

		// The ELF loader copies the binary's PT_TLS initial image into the
		// region that follows the TCB.
		let tls_image = &mut block[tls_offset..];
		let tls_init = func(code_slice, Some(tls_image))?;

		if let Some(init) = tls_init {
			let template =
				alloc::sync::Arc::new(crate::scheduler::task::TlsTemplate {
					size: tls_size as usize,
					init,
				});
			core_scheduler()
				.get_current_task()
				.borrow_mut()
				.tls_template = Some(template);
		}

		Ok(())
	} else {
		// No TLS in the freshly loaded image. We must still reset TPIDR_EL0
		// because an `exec()` re-enters this path: a stale value left over
		// from the previous program's TLS would otherwise persist across
		// the image swap and corrupt unrelated user-mode state on the
		// next thread-local access.
		set_user_tpidr_el0(0);
		func(code_slice, None)?;
		Ok(())
	}
}

/// Set the user-space `TPIDR_EL0` value for the *current* task.
///
/// The naive `msr tpidr_el0, xN` only changes the live register. When this
/// helper is called from inside an SVC handler — as it always is on the
/// `load_application` / `exec` path — `trap_exit` would later overwrite
/// `tpidr_el0` again from the value `trap_entry` saved in the on-stack
/// `State` struct (the trap-frame's `tpidr_el0` field). To make the new
/// thread pointer survive the trap-exit we therefore *also* update the
/// saved trap frame in place. The `State` lives at the very top of the
/// current kernel stack (`stack_top - MARKER_SIZE - sizeof(State)`).
#[cfg(feature = "common-os")]
fn set_user_tpidr_el0(value: u64) {
	use crate::arch::aarch64::kernel::scheduler::{State, TaskStacks};

	// Update the live system register so any pre-eret read in this kernel
	// path sees the new value.
	unsafe {
		asm!(
			"msr tpidr_el0, {0}",
			in(reg) value,
			options(nomem, nostack, preserves_flags),
		);
	}

	// Patch the saved trap frame so trap_exit restores the new value too.
	// `trap_entry` saved the user TPIDR_EL0 at SVC time; without this patch,
	// `trap_exit` would clobber our just-installed value with the stale one.
	let task = core_scheduler().get_current_task();
	let kernel_stack_top = task.borrow().stacks.get_kernel_stack().as_usize()
		+ task.borrow().stacks.get_kernel_stack_size();
	let state_addr = kernel_stack_top - TaskStacks::MARKER_SIZE - mem::size_of::<State>();
	unsafe {
		let state = ptr::with_exposed_provenance_mut::<State>(state_addr);
		(*state).tpidr_el0 = value;
	}
}

/// Drop into EL0, executing the freshly-loaded user binary at `entry_point`.
///
/// `iretq` on x86_64 corresponds to `eret` on AArch64: ELR_EL1 supplies the
/// new PC, SPSR_EL1 the new PSTATE (mode bits select EL0t), and SP_EL0 the
/// user stack. Per AAPCS64, `argc` lives in `x0` and `argv` in `x1`.
#[cfg(feature = "common-os")]
pub unsafe fn jump_to_user_land(entry_point: usize, code_size: usize, arg: &[&str]) -> ! {
	use alloc::ffi::CString;

	use align_address::Align;

	use crate::arch::aarch64::kernel::scheduler::TaskStacks;

	debug!("Create new file descriptor table");
	core_scheduler().recreate_objmap().unwrap();

	let entry_point: usize = LOADER_START | entry_point;
	let stack_top: usize = LOADER_START
		+ (code_size + LOADER_STACK_SIZE).align_up(BasePageSize::SIZE.try_into().unwrap())
		- TaskStacks::MARKER_SIZE;

	// Place the argv pointer array on the user stack.
	let stack_pointer = stack_top - arg.len() * mem::size_of::<*mut u8>();
	let stack_ptr = ptr::with_exposed_provenance_mut::<*mut u8>(stack_pointer);
	let argv = unsafe { slice::from_raw_parts_mut(stack_ptr, arg.len()) };
	let len = arg.iter().fold(0, |acc, x| acc + x.len() + 1);
	// AAPCS64 requires SP to be 16-byte aligned at function entry.
	let stack_pointer = (stack_pointer - len).align_down(16);

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

	debug!("Jump to user space at 0x{entry_point:x}, stack pointer 0x{stack_pointer:x}");

	// SPSR_EL1: M[4:0]=0b00000 ⇒ EL0t / AArch64; DAIF=0 ⇒ all interrupts enabled.
	const USER_SPSR: u64 = 0;

	unsafe {
		asm!(
			// Mask all interrupts during the EL1t→EL1h→EL0t transition.
			"msr daifset, #0b1111",
			// Switch to EL1h (SPSEL=1) so SP_EL0 is no longer the active SP
			// before we overwrite it. The initd task is started by `task_start`
			// in EL1t (SPSEL=0); writing SP_EL0 while it is the active stack
			// pointer is UNDEFINED on AArch64.
			"msr spsel, #1",
			"msr sp_el0,   {sp}",
			"msr elr_el1,  {pc}",
			"msr spsr_el1, {spsr}",
			"mov x0, {argc}",
			"mov x1, {argv}",
			// Clear scratch registers so EL1 state cannot leak into EL0.
			"mov x2,  xzr", "mov x3,  xzr", "mov x4,  xzr", "mov x5,  xzr",
			"mov x6,  xzr", "mov x7,  xzr", "mov x8,  xzr", "mov x9,  xzr",
			"mov x10, xzr", "mov x11, xzr", "mov x12, xzr", "mov x13, xzr",
			"mov x14, xzr", "mov x15, xzr", "mov x16, xzr", "mov x17, xzr",
			"mov x18, xzr", "mov x29, xzr", "mov x30, xzr",
			"eret",
			// Speculation barrier behind ERET (Spectre-v1 mitigation).
			"dsb nsh",
			"isb",
			sp   = in(reg) stack_pointer,
			pc   = in(reg) entry_point,
			spsr = in(reg) USER_SPSR,
			argc = in(reg) argv.len(),
			argv = in(reg) argv.as_ptr(),
			options(nostack, noreturn)
		);
	}
}
