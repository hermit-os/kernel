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
mod start;
pub mod switch;
pub mod systemtime;
#[cfg(feature = "common-os")]
use alloc::ffi::CString;
use alloc::vec::Vec;
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, Ordering};

use free_list::PageLayout;
use riscv::register::sstatus;

pub(crate) use self::processor::{set_oneshot_timer, wakeup_core};
use crate::arch::riscv64::kernel::core_local::core_id;
pub use crate::arch::riscv64::kernel::devicetree::init_drivers;
use crate::arch::riscv64::kernel::processor::lsb;
use crate::config::KERNEL_STACK_SIZE;
use crate::env;
use crate::init_cell::InitCell;
use crate::mm::{FrameAlloc, PageRangeAllocator};

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
	let fdt = env::fdt().unwrap();

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
	devicetree::init();
	crate::mm::init();
	crate::mm::print_information();
	interrupts::install();

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

	// The kernel writes user pages directly (program loading, argv/envp
	// setup, syscall buffers), which S-mode may only do with SUM set.
	#[cfg(feature = "common-os")]
	unsafe {
		sstatus::set_sum();
	}

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
	if env::is_uhyve() {
		return;
	}

	//When running bare-metal/QEMU we use the firmware to start the next hart
	let start_addr = (start::_start as *const ()).expose_provenance();
	sbi_rt::hart_start(next_hart_id as usize, start_addr, 0).unwrap();
}

pub fn print_statistics() {
	interrupts::print_statistics();
}

/// Start of the user-mode binary (Sv39 root slot 64, see
/// `mm::paging::USER_SPACE_START`).
#[cfg(feature = "common-os")]
pub(crate) const USER_START: memory_addresses::VirtAddr =
	memory_addresses::VirtAddr::new(crate::arch::riscv64::mm::paging::USER_SPACE_START as u64);
/// Top of the user stack: end of Sv39 root slot 191. The user heap
/// (`mm::vma::HEAP_START_ADDR`) starts at slot 128, so code, heap, and
/// stack live in disjoint 1 GiB root slots of the user region.
#[cfg(feature = "common-os")]
const USER_STACK: memory_addresses::VirtAddr =
	memory_addresses::VirtAddr::new(0x30_0000_0000 - USER_STACK_SIZE as u64);
#[cfg(feature = "common-os")]
const USER_STACK_SIZE: usize = 0x8000;

/// Thread-pointer values prepared by `load_application` for the first
/// user-mode entry of a task, keyed by task id. `jump_to_user_land`
/// consumes the entry when it builds the initial `UserContext`.
///
/// RISC-V has no privileged thread-pointer register (`tp` is a plain
/// GPR), so the value cannot be stashed in a CSR between the two calls.
#[cfg(feature = "common-os")]
static USER_TP: hermit_sync::InterruptTicketMutex<
	alloc::collections::BTreeMap<crate::scheduler::task::TaskId, u64>,
> = hermit_sync::InterruptTicketMutex::new(alloc::collections::BTreeMap::new());

/// Map the user-mode binary into the address space and run the ELF-loader
/// closure against the freshly-mapped pages. Mirrors the x86_64/aarch64
/// siblings.
#[allow(clippy::result_unit_err)]
#[cfg(feature = "common-os")]
pub fn load_application<F>(code_size: u64, tls_size: u64, func: F) -> Result<(), ()>
where
	F: FnOnce(&'static mut [u8], Option<&'static mut [u8]>) -> Result<Option<Vec<u8>>, ()>,
{
	use alloc::sync::Arc;

	use ahash::RandomState;
	use align_address::Align;
	use hashbrown::HashMap;
	use hermit_sync::RwSpinLock;
	use memory_addresses::PhysAddr;

	use crate::arch::riscv64::kernel::core_local::core_scheduler;
	use crate::arch::riscv64::mm::paging::{self, BasePageSize, PageSize, PageTableEntryFlags};
	use crate::fd::{Fd, RawFd};
	use crate::mm::vma::*;

	// Each process has its own object map.
	let mut object_map = HashMap::<RawFd, Arc<async_lock::RwLock<Fd>>, RandomState>::with_hasher(
		RandomState::with_seeds(0, 0, 0, 0),
	);
	crate::fd::stdio::setup(&mut object_map);
	core_scheduler().set_current_task_object_map(Arc::new(RwSpinLock::new(object_map)));

	let code_size = (code_size as usize).align_up(BasePageSize::SIZE as usize);
	let layout = PageLayout::from_size_align(code_size, BasePageSize::SIZE as usize).unwrap();
	let frame_range = FrameAlloc::allocate(layout).unwrap();
	let physaddr = PhysAddr::from(frame_range.start());

	let mut flags = PageTableEntryFlags::empty();
	flags.normal().writable().user();
	paging::map::<BasePageSize>(
		USER_START,
		physaddr,
		code_size / BasePageSize::SIZE as usize,
		flags,
	);
	core_scheduler()
		.get_current_task()
		.borrow_mut()
		.vmas
		.write()
		.insert(
			USER_START,
			VirtualMemoryArea::new(
				USER_START,
				(USER_START + code_size).align_up(BasePageSize::SIZE),
				VirtualMemoryAreaProt::READ
					| VirtualMemoryAreaProt::WRITE
					| VirtualMemoryAreaProt::EXECUTE,
				MemoryType::CODE,
			),
		);

	let loader_start_ptr = ptr::with_exposed_provenance_mut(USER_START.as_usize());
	let code_slice = unsafe { core::slice::from_raw_parts_mut(loader_start_ptr, code_size) };

	let current_task_id = core_scheduler().get_current_task().borrow().id;

	if tls_size > 0 {
		// RISC-V uses TLS Variant I: `tp` points at the first byte of the
		// TLS data; the two-word TCB sits directly below it. We allocate
		// the TCB plus the TLS image as one contiguous block.
		let tcb_size = 2 * size_of::<*mut ()>();
		let tls_offset = tcb_size;

		let tls_memsz = (tls_offset + tls_size as usize).align_up(BasePageSize::SIZE as usize);
		let layout = PageLayout::from_size(tls_memsz).unwrap();
		let frame_range = FrameAlloc::allocate(layout).unwrap();
		let physaddr = PhysAddr::from(frame_range.start());

		let mut flags = PageTableEntryFlags::empty();
		flags.normal().writable().user().execute_disable();
		let tls_virt = USER_START + code_size as u64 + BasePageSize::SIZE;
		paging::map::<BasePageSize>(
			tls_virt,
			physaddr,
			tls_memsz / BasePageSize::SIZE as usize,
			flags,
		);
		core_scheduler()
			.get_current_task()
			.borrow_mut()
			.vmas
			.write()
			.insert(
				tls_virt,
				VirtualMemoryArea::new(
					tls_virt,
					(tls_virt + tls_memsz as u64).align_up(BasePageSize::SIZE),
					VirtualMemoryAreaProt::READ | VirtualMemoryAreaProt::WRITE,
					MemoryType::TLS,
				),
			);

		let block = unsafe {
			core::slice::from_raw_parts_mut(
				tls_virt.as_mut_ptr::<u8>(),
				tls_offset + tls_size as usize,
			)
		};
		for elem in block.iter_mut() {
			*elem = 0;
		}

		// Variant I: `tp` points at the TLS data that follows the TCB.
		USER_TP
			.lock()
			.insert(current_task_id, (tls_virt + tls_offset as u64).as_u64());

		// The ELF loader copies the binary's PT_TLS initial image into the
		// region that follows the TCB.
		let tls_image = &mut block[tls_offset..];
		let tls_init = func(code_slice, Some(tls_image))?;

		if let Some(init) = tls_init {
			let template = Arc::new(crate::scheduler::task::TlsTemplate {
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
		// No TLS in the freshly loaded image. Drop any stale value left
		// over from a previous image (`exec` re-enters this path).
		USER_TP.lock().remove(&current_task_id);
		func(code_slice, None)?;
		Ok(())
	}
}

/// Run `ctx` in user mode until the next trap, preserving the kernel's
/// `gp` register.
///
/// The trapframe crate's user-trap return path restores the callee-saved
/// registers, `ra`, and `tp` — but not `gp`, which this kernel uses as
/// the `CoreLocal` pointer. Interrupts stay disabled from trap entry
/// until after `gp` is restored, so no kernel code can observe the
/// user's `gp` value.
#[cfg(feature = "common-os")]
fn run_user(ctx: &mut trapframe::UserContext) {
	let gp: usize;
	unsafe {
		core::arch::asm!("mv {}, gp", out(reg) gp);
	}
	ctx.run();
	unsafe {
		core::arch::asm!("mv gp, {}", in(reg) gp);
	}
}

/// Handle a page fault raised in user mode: fault in one page of the
/// surrounding virtual memory area, if any.
///
/// Returns `true` if the fault was resolved and the faulting instruction
/// can be retried.
#[cfg(feature = "common-os")]
pub(crate) fn do_user_page_fault(fault_addr: usize) -> bool {
	use core::ops::Bound;

	use align_address::Align;
	use memory_addresses::{PhysAddr, VirtAddr};

	use crate::arch::riscv64::kernel::core_local::core_scheduler;
	use crate::arch::riscv64::mm::paging::{self, BasePageSize, PageSize, PageTableEntryFlags};
	use crate::mm::vma::VirtualMemoryAreaProt;

	let addr = VirtAddr::new(fault_addr as u64).align_down(BasePageSize::SIZE);
	let current_task = core_scheduler().get_current_task();
	let current_task_borrowed = current_task.borrow();
	let guard = current_task_borrowed.vmas.read();

	if let Some((_, vma)) = guard
		.range((Bound::Unbounded, Bound::Included(addr)))
		.next_back()
		&& addr >= vma.start
		&& addr < vma.end
	{
		let layout =
			PageLayout::from_size_align(BasePageSize::SIZE as usize, BasePageSize::SIZE as usize)
				.unwrap();
		let frame_range = FrameAlloc::allocate(layout).unwrap();
		let physaddr = PhysAddr::from(frame_range.start());

		let mut flags = PageTableEntryFlags::empty();
		flags.normal().user();
		if vma.prot.contains(VirtualMemoryAreaProt::WRITE) {
			flags.writable();
		}
		if !vma.prot.contains(VirtualMemoryAreaProt::EXECUTE) {
			flags.execute_disable();
		}

		paging::map::<BasePageSize>(addr, physaddr, 1, flags);

		// Clear the page through the identity mapping of physical memory,
		// so this works independently of `sstatus.SUM`.
		let slice = unsafe {
			core::slice::from_raw_parts_mut(
				ptr::with_exposed_provenance_mut::<u8>(physaddr.as_usize()),
				BasePageSize::SIZE as usize,
			)
		};
		slice.fill(0);

		return true;
	}

	false
}

/// Dispatch a system call raised by `ecall` from user mode.
///
/// Calling convention (mirrors the x86_64/aarch64 variants of this
/// kernel): syscall number in `a7`, up to six arguments in `a0`..`a5`,
/// return value in `a0`.
#[cfg(feature = "common-os")]
fn dispatch_syscall(ctx: &mut trapframe::UserContext) {
	use crate::syscalls::table::{NO_SYSCALLS, SYSHANDLER_TABLE, sys_invalid};

	// Resume execution after the 4-byte `ecall` instruction.
	ctx.sepc += 4;

	let nr = ctx.general.a7;
	if nr >= NO_SYSCALLS {
		crate::syscalls::table::invalid_syscall(nr as u64);
	}

	let handler_ptr = SYSHANDLER_TABLE.handler(nr);
	if handler_ptr == sys_invalid as *const usize {
		crate::syscalls::table::invalid_syscall(nr as u64);
	}

	let f: extern "C" fn(u64, u64, u64, u64, u64, u64) -> u64 =
		unsafe { core::mem::transmute(handler_ptr) };

	// The handlers may block; run them with interrupts enabled like the
	// x86_64 syscall path does.
	unsafe { sstatus::set_sie() };
	let result = f(
		ctx.general.a0 as u64,
		ctx.general.a1 as u64,
		ctx.general.a2 as u64,
		ctx.general.a3 as u64,
		ctx.general.a4 as u64,
		ctx.general.a5 as u64,
	);
	unsafe { sstatus::clear_sie() };

	ctx.general.a0 = result as usize;
}

/// Enter user mode at `entry` with the given stack pointer, thread
/// pointer, and argument registers, and service all traps the user code
/// raises. This function never returns: the process leaves through the
/// `exit` system call (or is torn down after a fatal fault).
#[cfg(feature = "common-os")]
pub(crate) fn user_loop(
	entry: usize,
	stack_pointer: usize,
	thread_pointer: u64,
	args: [usize; 3],
) -> ! {
	// sstatus value for user-mode entry, derived from the current state:
	// SPP = User, SPIE = 1 (interrupts enabled after `sret`), SUM = 1
	// (syscall handlers access user buffers), FS = Initial (user code may
	// use the FPU; the scheduler tracks the dirty state).
	const SSTATUS_SPIE: usize = 1 << 5;
	const SSTATUS_SPP: usize = 1 << 8;
	const SSTATUS_FS_MASK: usize = 0b11 << 13;
	const SSTATUS_FS_INITIAL: usize = 0b01 << 13;
	const SSTATUS_SUM: usize = 1 << 18;

	let sstatus_raw: usize;
	unsafe {
		core::arch::asm!("csrr {}, sstatus", out(reg) sstatus_raw);
	}
	let user_sstatus = (sstatus_raw & !(SSTATUS_SPP | SSTATUS_FS_MASK))
		| SSTATUS_SPIE
		| SSTATUS_FS_INITIAL
		| SSTATUS_SUM;

	let mut ctx = trapframe::UserContext {
		sepc: entry,
		sstatus: user_sstatus,
		..Default::default()
	};
	ctx.general.sp = stack_pointer;
	ctx.general.tp = thread_pointer as usize;
	ctx.general.a0 = args[0];
	ctx.general.a1 = args[1];
	ctx.general.a2 = args[2];

	debug!("Jump to user space at {entry:#x}, stack pointer {stack_pointer:#x}");

	user_loop_resume(ctx)
}

/// Service all traps the user code raises, starting user execution
/// from `ctx`. Never returns; the process leaves through the `exit`
/// system call (or is torn down after a fatal fault).
#[cfg(feature = "common-os")]
pub(crate) fn user_loop_resume(mut ctx: trapframe::UserContext) -> ! {
	use riscv::interrupt::{Exception, Interrupt, Trap};
	use riscv::register::{scause, stval};

	use crate::arch::riscv64::kernel::core_local::core_scheduler;
	use crate::scheduler::PerCoreSchedulerExt;

	loop {
		run_user(&mut ctx);

		let scause = scause::read();
		let cause = Trap::<Interrupt, Exception>::try_from(scause.cause()).unwrap();

		match cause {
			Trap::Exception(Exception::UserEnvCall) => {
				dispatch_syscall(&mut ctx);
			}
			Trap::Exception(
				Exception::InstructionPageFault
				| Exception::LoadPageFault
				| Exception::StorePageFault,
			) => {
				let fault_addr = stval::read();

				if !do_user_page_fault(fault_addr) {
					error!("Unhandled user page fault at {fault_addr:#x} ({cause:?})");
					error!("sepc = {:#x}", ctx.sepc);
					core_scheduler().exit(1);
				}
			}
			Trap::Interrupt(Interrupt::SupervisorTimer) => {
				scheduler::timer_handler();
			}
			Trap::Interrupt(Interrupt::SupervisorExternal) => {
				interrupts::external_handler();
			}
			#[cfg(feature = "smp")]
			Trap::Interrupt(Interrupt::SupervisorSoft) => {
				scheduler::wakeup_handler();
			}
			cause => {
				error!("Fatal user trap: {cause:?}");
				error!("sepc = {:#x}, stval = {:#x}", ctx.sepc, stval::read());
				core_scheduler().exit(1);
			}
		}
	}
}

/// Jump into the user-space application that `load_application` placed at
/// `USER_START`.
#[cfg(feature = "common-os")]
pub unsafe fn jump_to_user_land(entry_point: usize, args: Vec<CString>, envs: Vec<CString>) -> ! {
	use align_address::Align;
	use memory_addresses::PhysAddr;

	use crate::arch::riscv64::kernel::core_local::core_scheduler;
	use crate::arch::riscv64::mm::paging::{self, BasePageSize, PageSize, PageTableEntryFlags};
	use crate::mm::vma::*;

	debug!("Create new file descriptor table");
	core_scheduler().recreate_objmap().unwrap();

	let entry_point: usize = USER_START.as_usize() | entry_point;
	let stack_top: usize = USER_STACK.as_usize() + USER_STACK_SIZE;

	let layout = PageLayout::from_size(USER_STACK_SIZE).unwrap();
	let frame_range = FrameAlloc::allocate(layout).unwrap();
	let phys_addr = PhysAddr::from(frame_range.start());
	let mut flags = PageTableEntryFlags::empty();
	flags.normal().writable().user().execute_disable();
	paging::map::<BasePageSize>(
		USER_STACK,
		phys_addr,
		USER_STACK_SIZE / BasePageSize::SIZE as usize,
		flags,
	);
	core_scheduler()
		.get_current_task()
		.borrow_mut()
		.vmas
		.write()
		.insert(
			USER_STACK,
			VirtualMemoryArea::new(
				USER_STACK,
				USER_STACK + USER_STACK_SIZE as u64,
				VirtualMemoryAreaProt::READ | VirtualMemoryAreaProt::WRITE,
				MemoryType::STACK,
			),
		);

	// Place the argv and envp pointer arrays on the user stack. Both
	// arrays follow the C convention and are terminated by a null pointer.
	// The kernel accesses the user stack through `sstatus.SUM` (set at
	// boot for common-os).
	let ptr_count = args.len() + 1 + envs.len() + 1;
	let stack_pointer = stack_top - ptr_count * size_of::<*mut u8>();
	let stack_ptr = ptr::with_exposed_provenance_mut::<*mut u8>(stack_pointer);
	let arrays = unsafe { core::slice::from_raw_parts_mut(stack_ptr, ptr_count) };
	let (argv, envp) = arrays.split_at_mut(args.len() + 1);
	let len = args
		.iter()
		.chain(envs.iter())
		.fold(0, |acc, x| acc + x.as_bytes_with_nul().len());
	// The RISC-V psABI requires SP to be 16-byte aligned at function entry.
	let stack_pointer = (stack_pointer - len).align_down(16);

	let mut pos: usize = 0;
	for (dst, s) in argv
		.iter_mut()
		.zip(args.iter())
		.chain(envp.iter_mut().zip(envs.iter()))
	{
		let bytes = s.as_bytes_with_nul();
		*dst = ptr::with_exposed_provenance_mut::<u8>(stack_pointer + pos);
		pos += bytes.len();

		unsafe {
			dst.copy_from_nonoverlapping(bytes.as_ptr(), bytes.len());
		}
	}
	argv[args.len()] = ptr::null_mut();
	envp[envs.len()] = ptr::null_mut();

	let argc = args.len();
	let argv_ptr = argv.as_ptr().expose_provenance();
	let envp_ptr = envp.as_ptr().expose_provenance();
	drop(args);
	drop(envs);

	let current_task_id = core_scheduler().get_current_task().borrow().id;
	let thread_pointer = USER_TP.lock().remove(&current_task_id).unwrap_or_default();

	user_loop(
		entry_point,
		stack_pointer,
		thread_pointer,
		[argc, argv_ptr, envp_ptr],
	)
}
