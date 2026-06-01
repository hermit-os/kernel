//! Architecture dependent interface to initialize a task

use core::arch::naked_asm;
use core::sync::atomic::Ordering;
use core::{mem, ptr};

use aarch64_cpu::asm::barrier::{SY, dsb, isb};
use aarch64_cpu::registers::*;
use align_address::Align;
use free_list::{PageLayout, PageRange};
use memory_addresses::{PhysAddr, VirtAddr};

use crate::arch::aarch64::kernel::CURRENT_STACK_ADDRESS;
use crate::arch::aarch64::kernel::core_local::core_scheduler;
use crate::arch::aarch64::mm::paging::{BasePageSize, PageSize, PageTableEntryFlags};
use crate::config::{DEFAULT_STACK_SIZE, KERNEL_STACK_SIZE};
use crate::mm::{FrameAlloc, PageAlloc, PageRangeAllocator};
use crate::scheduler::PerCoreSchedulerExt;
use crate::scheduler::task::{Task, TaskFrame};

#[derive(Debug)]
#[repr(C, packed)]
pub(crate) struct State {
	/// Stack selector
	pub spsel: u64,
	/// Exception Link Register
	pub elr_el1: extern "C" fn(extern "C" fn(usize), usize) -> !,
	/// Program Status Register
	pub spsr_el1: u64,
	/// User-level stack
	pub sp_el0: u64,
	/// Thread ID Register
	pub tpidr_el0: u64,
	/// X0 register
	pub x0: u64,
	/// X1 register
	pub x1: u64,
	/// X2 register
	pub x2: u64,
	/// X3 register
	pub x3: u64,
	/// X4 register
	pub x4: u64,
	/// X5 register
	pub x5: u64,
	/// X6 register
	pub x6: u64,
	/// X7 register
	pub x7: u64,
	/// X8 register
	pub x8: u64,
	/// X9 register
	pub x9: u64,
	/// X10 register
	pub x10: u64,
	/// X11 register
	pub x11: u64,
	/// X12 register
	pub x12: u64,
	/// X13 register
	pub x13: u64,
	/// X14 register
	pub x14: u64,
	/// X15 register
	pub x15: u64,
	/// X16 register
	pub x16: u64,
	/// X17 register
	pub x17: u64,
	/// X18 register
	pub x18: u64,
	/// X19 register
	pub x19: u64,
	/// X20 register
	pub x20: u64,
	/// X21 register
	pub x21: u64,
	/// X22 register
	pub x22: u64,
	/// X23 register
	pub x23: u64,
	/// X24 register
	pub x24: u64,
	/// X25 register
	pub x25: u64,
	/// X26 register
	pub x26: u64,
	/// X27 register
	pub x27: u64,
	/// X28 register
	pub x28: u64,
	/// X29 register
	pub x29: u64,
	/// X30 register
	pub x30: u64,
}

pub struct BootStack {
	/// Stack for kernel tasks
	stack: VirtAddr,
}

pub struct CommonStack {
	/// Start address of allocated virtual memory region
	virt_addr: VirtAddr,
	/// Start address of allocated virtual memory region
	phys_addr: PhysAddr,
	/// Total size of all stacks
	total_size: usize,
}

pub enum TaskStacks {
	Boot(BootStack),
	Common(CommonStack),
}

impl TaskStacks {
	/// Size of the debug marker at the very top of each stack.
	///
	/// We have a marker at the very top of the stack for debugging (`0xdeadbeef`), which should not be overridden.
	pub const MARKER_SIZE: usize = 0x10;

	pub fn new(size: usize) -> Self {
		let user_stack_size = if size < KERNEL_STACK_SIZE {
			KERNEL_STACK_SIZE
		} else {
			size.align_up(BasePageSize::SIZE as usize)
		};
		let total_size = user_stack_size + DEFAULT_STACK_SIZE;
		let layout = PageLayout::from_size(total_size + 3 * BasePageSize::SIZE as usize).unwrap();
		let page_range = PageAlloc::allocate(layout).unwrap();
		let virt_addr = VirtAddr::from(page_range.start());
		let frame_layout = PageLayout::from_size(total_size).unwrap();
		let frame_range = FrameAlloc::allocate(frame_layout)
			.expect("Failed to allocate Physical Memory for TaskStacks");
		let phys_addr = PhysAddr::from(frame_range.start());

		debug!(
			"Create stacks at {:p} with a size of {} KB",
			virt_addr,
			total_size >> 10
		);

		let mut kernel_flags = PageTableEntryFlags::empty();
		kernel_flags.normal().writable().execute_disable();

		// map kernel stack into the address space (kernel-only access)
		crate::arch::mm::paging::map::<BasePageSize>(
			virt_addr + BasePageSize::SIZE,
			phys_addr,
			DEFAULT_STACK_SIZE / BasePageSize::SIZE as usize,
			kernel_flags,
		);

		// User-stack flags differ between unikernel and common-os builds:
		// in common-os the same VA is reachable from both EL1 (the kernel
		// crafting argv during jump_to_user_land) and EL0 (the running
		// thread), so it must carry USER_ACCESSIBLE. Without this, a
		// freshly spawned user thread (`scheduler::spawn_thread`) faults
		// as soon as it touches its own stack — TaskStacks::new is the
		// only path that allocates a user stack outside the LOADER_START
		// region, so the bug only manifests on the thread-spawn path.
		#[cfg(feature = "common-os")]
		let user_flags = {
			let mut f = PageTableEntryFlags::empty();
			f.normal().writable().user().execute_disable();
			f
		};
		#[cfg(not(feature = "common-os"))]
		let user_flags = kernel_flags;

		// map user stack into the address space
		crate::arch::mm::paging::map::<BasePageSize>(
			virt_addr + DEFAULT_STACK_SIZE + 2 * BasePageSize::SIZE,
			phys_addr + DEFAULT_STACK_SIZE,
			user_stack_size / BasePageSize::SIZE as usize,
			user_flags,
		);

		// clear user stack
		unsafe {
			(virt_addr + DEFAULT_STACK_SIZE + 2 * BasePageSize::SIZE)
				.as_mut_ptr::<u8>()
				.write_bytes(0, user_stack_size);
		}

		TaskStacks::Common(CommonStack {
			virt_addr,
			phys_addr,
			total_size,
		})
	}

	pub fn from_boot_stacks() -> TaskStacks {
		let stack = VirtAddr::from_ptr(CURRENT_STACK_ADDRESS.load(Ordering::Relaxed));
		debug!("Using boot stack {stack:p}");

		TaskStacks::Boot(BootStack { stack })
	}

	pub fn get_user_stack_size(&self) -> usize {
		match self {
			TaskStacks::Boot(_) => 0,
			TaskStacks::Common(stacks) => stacks.total_size - DEFAULT_STACK_SIZE,
		}
	}

	/// Returns the start address of the stack region (virt_addr of CommonStack).
	#[cfg(feature = "common-os")]
	pub fn get_stack_virt_addr(&self) -> VirtAddr {
		match self {
			TaskStacks::Boot(stacks) => stacks.stack,
			TaskStacks::Common(stacks) => stacks.virt_addr,
		}
	}

	/// Returns total size of all stacks combined.
	#[cfg(feature = "common-os")]
	pub fn get_total_stack_size(&self) -> usize {
		match self {
			TaskStacks::Boot(_) => KERNEL_STACK_SIZE,
			TaskStacks::Common(stacks) => stacks.total_size,
		}
	}

	pub fn get_user_stack(&self) -> VirtAddr {
		match self {
			TaskStacks::Boot(_) => VirtAddr::zero(),
			TaskStacks::Common(stacks) => {
				stacks.virt_addr + DEFAULT_STACK_SIZE + 2 * BasePageSize::SIZE
			}
		}
	}

	pub fn get_kernel_stack(&self) -> VirtAddr {
		match self {
			TaskStacks::Boot(stacks) => stacks.stack,
			TaskStacks::Common(stacks) => stacks.virt_addr + BasePageSize::SIZE,
		}
	}

	pub fn get_kernel_stack_size(&self) -> usize {
		match self {
			TaskStacks::Boot(_) => KERNEL_STACK_SIZE,
			TaskStacks::Common(_) => DEFAULT_STACK_SIZE,
		}
	}
}

impl Drop for TaskStacks {
	fn drop(&mut self) {
		// we should never deallocate a boot stack
		match self {
			TaskStacks::Boot(_) => {}
			TaskStacks::Common(stacks) => {
				debug!(
					"Deallocating stacks at {:p} with a size of {} KB",
					stacks.virt_addr,
					stacks.total_size >> 10,
				);

				crate::arch::mm::paging::unmap::<BasePageSize>(
					stacks.virt_addr,
					stacks.total_size / BasePageSize::SIZE as usize + 3,
				);
				let range = PageRange::from_start_len(
					stacks.virt_addr.as_usize(),
					stacks.total_size + 3 * BasePageSize::SIZE as usize,
				)
				.unwrap();
				unsafe {
					PageAlloc::deallocate(range);
				}

				let range =
					PageRange::from_start_len(stacks.phys_addr.as_usize(), stacks.total_size)
						.unwrap();
				unsafe {
					FrameAlloc::deallocate(range);
				}
			}
		}
	}
}

/*
 * https://fuchsia.dev/fuchsia-src/development/kernel/threads/tls and
 * and https://uclibc.org/docs/tls.pdf is used to understand variant 1
 * of the TLS implementations.
 */

extern "C" fn thread_exit(status: i32) -> ! {
	debug!("Exit thread with error code {status}!");
	core_scheduler().exit(status)
}

#[unsafe(naked)]
extern "C" fn task_start(_f: extern "C" fn(usize), _arg: usize) -> ! {
	// `f` is in the `x0` register
	// `arg` is in the `x1` register

	naked_asm!(
		"msr spsel, {l0}",
		"mov x25, x0",
		"mov x0, x1",
		"blr x25",
		"mov x0, xzr",
		"adrp x4, {exit}",
		"add x4, x4, #:lo12:{exit}",
		"br x4",
		l0 = const 0,
		exit = sym thread_exit,
	)
}

#[cfg(feature = "common-os")]
impl Task {
	/// Build the initial trap frame for a freshly spawned user-space
	/// thread. Mirrors the role of the x86_64 sibling: when the scheduler
	/// first picks this task, the standard `trap_exit` machinery pops the
	/// `State` we craft here and `eret`s straight into ring 3 at
	/// `func(arg)` on the new user stack — so no naked-asm "task_start_user"
	/// trampoline is needed on AArch64.
	///
	/// `tls_thread_ptr` is the value that should be installed in
	/// `TPIDR_EL0` for the new thread (the per-thread TLS thread pointer
	/// allocated by `scheduler::allocate_thread_tls`); zero leaves the
	/// register as installed by the loader.
	pub(crate) fn create_user_stack_frame(
		&mut self,
		func: unsafe extern "C" fn(usize),
		arg: usize,
		tls_thread_ptr: u64,
	) {
		unsafe {
			// Debug marker at the very top of the kernel stack.
			let mut stack = self.stacks.get_kernel_stack()
				+ self.stacks.get_kernel_stack_size()
				- TaskStacks::MARKER_SIZE;
			*stack.as_mut_ptr::<u64>() = 0xdead_beefu64;

			// Allocate space for the trap frame and zero it. Anything we
			// don't touch below stays zero on entry to user space, which
			// keeps any leftover kernel state out of EL0's general-purpose
			// register file.
			stack -= size_of::<State>();
			let state = stack.as_mut_ptr::<State>();
			ptr::write_bytes(state.cast::<u8>(), 0, size_of::<State>());

			// Initial user stack: top of the user-stack region with the
			// usual debug marker. AAPCS64 doesn't require any extra slop
			// (no red zone, no shadow space), so the user starts at SP
			// pointing at the byte immediately above the marker.
			self.user_stack_pointer = self.stacks.get_user_stack()
				+ self.stacks.get_user_stack_size()
				- TaskStacks::MARKER_SIZE;
			*self.user_stack_pointer.as_mut_ptr::<u64>() = 0xdead_beefu64;

			(*state).elr_el1 = mem::transmute::<
				unsafe extern "C" fn(usize),
				extern "C" fn(extern "C" fn(usize), usize) -> !,
			>(func);
			// SPSR_EL1 = 0 ⇒ M[4:0] = 0b00000 (EL0t / AArch64), DAIF = 0
			// (interrupts unmasked once the thread is running).
			(*state).spsr_el1 = 0;
			(*state).sp_el0 = self.user_stack_pointer.as_u64();
			(*state).tpidr_el0 = tls_thread_ptr;
			// AAPCS64 first argument register.
			(*state).x0 = arg as u64;
			// SPSEL is consumed by trap_exit but does not affect the
			// post-eret EL0 SP selection (SPSR_EL1 alone determines it).
			(*state).spsel = 1;

			self.last_stack_pointer = stack;
		}
	}
}

impl TaskFrame for Task {
	fn create_stack_frame(&mut self, func: unsafe extern "C" fn(usize), arg: usize) {
		// Check if TLS is allocated already and if the task uses thread-local storage.
		#[cfg(not(feature = "common-os"))]
		if self.tls.is_none() {
			use crate::scheduler::task::tls::Tls;

			self.tls = Tls::from_env();
		}

		unsafe {
			// Set a marker for debugging at the very top.
			let mut stack = self.stacks.get_kernel_stack() + self.stacks.get_kernel_stack_size()
				- TaskStacks::MARKER_SIZE;
			*stack.as_mut_ptr::<u64>() = 0xdead_beefu64;

			// Put the State structure expected by the ASM switch() function on the stack.
			stack -= size_of::<State>();

			let state = stack.as_mut_ptr::<State>();
			#[cfg(not(feature = "common-os"))]
			if let Some(tls) = &self.tls {
				(*state).tpidr_el0 = tls.thread_ptr().expose_provenance() as u64;
			}

			// The elr_el1 needs to hold the address of the
			// first function to be called when returning from exception handler.
			(*state).elr_el1 = task_start;
			(*state).x0 = func as usize as u64; // use second argument to transfer the entry point
			(*state).x1 = arg as u64;
			(*state).spsel = 1;

			/* Zero the condition flags. */
			(*state).spsr_el1 = 0x3e5;

			// Set the task's stack pointer entry to the stack we have just crafted.
			self.last_stack_pointer = stack;

			// initialize user-level stack
			self.user_stack_pointer = self.stacks.get_user_stack()
				+ self.stacks.get_user_stack_size()
				- TaskStacks::MARKER_SIZE;
			*self.user_stack_pointer.as_mut_ptr::<u64>() = 0xdead_beefu64;
			(*state).sp_el0 = self.user_stack_pointer.as_u64();
		}
	}
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn get_last_stack_pointer() -> u64 {
	use aarch64_cpu::asm::barrier::{ISH, ISHST};

	// Trap next FPU instruction so we can lazily restore FPU state
	CPACR_EL1.modify(CPACR_EL1::FPEN::TrapEl0El1);
	isb(SY);

	let scheduler = core_scheduler();

	// Switch TTBR0_EL1 to the new task's root page table when the address
	// space changes between two `common-os` processes. The IRQ-driven
	// context switch (see `start.s`) calls this helper after the scheduler
	// has already promoted `current_task` to the new task; if we leave
	// TTBR0_EL1 pointing at the previous process's table, the new task
	// runs in the OLD address space — which becomes catastrophic the
	// moment that task issues `clear_user_space` (it clears the wrong
	// mapping) or its user code touches its own TLS.
	#[cfg(feature = "common-os")]
	{
		let new_pt = scheduler
			.get_current_task()
			.borrow()
			.root_page_table
			.as_usize() as u64;
		let cur_pt = TTBR0_EL1.get_baddr();
		if cur_pt != new_pt {
			// Memory-barrier sequence per ARM ARM D8.13.2: DSB ISHST
			// ensures all prior PT updates are observable; the MSR
			// installs the new translation base; ISB flushes the
			// pipeline so subsequent instructions use the new table.
			dsb(ISHST);
			TTBR0_EL1.set_baddr(new_pt);
			isb(SY);
			// Invalidate TLB entries from the old translation regime.
			unsafe {
				core::arch::asm!("tlbi vmalle1is", options(nostack));
			}
			dsb(ISH);
			isb(SY);
		}
	}

	scheduler.get_last_stack_pointer().as_u64()
}

/// Prepare the child's stack and root page table for a fork(), AArch64.
///
/// Mirrors the role of the x86_64 `prepare_fork_child_stack`, but does not
/// need a naked-asm child-entry stub: when the SVC trapped into EL1, the
/// hardware-supplied `trap_entry` macro pushed a complete `State` struct
/// at the top of the parent's kernel stack. Copying the kernel stack page
/// for the child duplicates that `State`; if we then patch `x0 = 0` in
/// the child's copy, the existing trap-exit machinery will `eret` it
/// straight back to the user-space instruction after the SVC with the
/// fork-returns-zero contract satisfied.
///
/// Operations performed (in order; ordering matters):
/// 1. Copy the parent's kernel stack pages into `new_stack_addr`. This
///    runs in the parent's still-active page table, so the new mappings
///    become visible immediately.
/// 2. Snapshot the current root page table for the child via
///    `copy_current_root_page_table` — the snapshot now includes the
///    just-copied stack mapping.
/// 3. Patch the child's saved-`x0` to 0.
/// 4. Compute the child's saved kernel-SP (the address of the child's
///    `State` copy) and store it through `stack_pointer`.
///
/// Returns `false` (the parent path); the child path becomes reachable
/// once the scheduler context-switches to the new task and the existing
/// IRQ trap-exit pops the child's `State` and `eret`s.
#[cfg(all(feature = "common-os", feature = "fork"))]
pub unsafe fn prepare_fork_child_stack(
	stack_pointer: *mut usize,
	root_page_table: *mut usize,
	new_stack_addr: usize,
) -> bool {
	use crate::arch::aarch64::mm::{copy_current_root_page_table, copy_kernel_stack_to};

	// 1. Copy the kernel stack pages into the child's region. Must run
	//    before the page-table snapshot so the new mappings are visible.
	copy_kernel_stack_to(new_stack_addr);

	// 2. Duplicate the root page table for the child and hand the new
	//    physical address back to the caller.
	let new_pt = copy_current_root_page_table();
	unsafe { *root_page_table = new_pt };

	// 3. Locate the parent's saved `State` (the structure `trap_entry`
	//    pushed at SVC time) and compute the matching address inside the
	//    child's freshly-copied kernel stack.
	let task = core_scheduler().get_current_task();
	let parent_stack_base = task.borrow().stacks.get_stack_virt_addr().as_usize();
	let kernel_stack_top = task.borrow().stacks.get_kernel_stack().as_usize()
		+ task.borrow().stacks.get_kernel_stack_size();
	let parent_state_addr = kernel_stack_top - TaskStacks::MARKER_SIZE - size_of::<State>();
	let offset = new_stack_addr.wrapping_sub(parent_stack_base);
	let child_state_addr = parent_state_addr.wrapping_add(offset);

	// 4. Patch the child's `x0` so fork() returns 0 there. The rest of
	//    the State (ELR_EL1 = post-SVC user PC, SPSR_EL1 = EL0t, SP_EL0
	//    = user stack, x1..x30 = parent's user regs) is already correct.
	unsafe {
		let state = ptr::with_exposed_provenance_mut::<State>(child_state_addr);
		(*state).x0 = 0;
	}

	// 5. Hand the child's kernel SP back to the caller; the scheduler
	//    will plug it into the child's task struct as `last_stack_pointer`.
	unsafe { *stack_pointer = child_state_addr };

	false
}
