//! Architecture dependent interface to initialize a task

use core::arch::naked_asm;
use core::sync::atomic::Ordering;
use core::{mem, ptr};

use aarch64_cpu::asm::barrier::{SY, isb};
use aarch64_cpu::registers::*;
use align_address::Align;
use free_list::{PageLayout, PageRange};
use memory_addresses::arch::aarch64::{PhysAddr, VirtAddr};

use crate::arch::aarch64::kernel::CURRENT_STACK_ADDRESS;
use crate::arch::aarch64::kernel::core_local::core_scheduler;
use crate::arch::aarch64::mm::paging::{BasePageSize, PageSize, PageTableEntryFlags};
use crate::mm::{FrameAlloc, PageAlloc, PageRangeAllocator};
#[cfg(target_os = "none")]
use crate::scheduler::PerCoreSchedulerExt;
use crate::scheduler::task::{Task, TaskFrame};
use crate::{DEFAULT_STACK_SIZE, KERNEL_STACK_SIZE};

#[derive(Debug)]
#[repr(C, packed)]
pub(crate) struct State {
	/// stack selector
	pub spsel: u64,
	/// Exception Link Register
	pub elr_el1: u64,
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
	/// stack for kernel tasks
	stack: VirtAddr,
}

pub struct CommonStack {
	/// start address of allocated virtual memory region
	virt_addr: VirtAddr,
	/// start address of allocated virtual memory region
	phys_addr: PhysAddr,
	/// total size of all stacks
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

		let mut flags = PageTableEntryFlags::empty();
		flags.normal().writable().execute_disable();

		// map kernel stack into the address space
		crate::arch::mm::paging::map::<BasePageSize>(
			virt_addr + BasePageSize::SIZE,
			phys_addr,
			DEFAULT_STACK_SIZE / BasePageSize::SIZE as usize,
			flags,
		);

		// map user stack into the address space
		crate::arch::mm::paging::map::<BasePageSize>(
			virt_addr + DEFAULT_STACK_SIZE + 2 * BasePageSize::SIZE,
			phys_addr + DEFAULT_STACK_SIZE,
			user_stack_size / BasePageSize::SIZE as usize,
			flags,
		);

		// clear user stack
		unsafe {
			ptr::write_bytes(
				(virt_addr + DEFAULT_STACK_SIZE + 2 * BasePageSize::SIZE).as_mut_ptr::<u8>(),
				0,
				user_stack_size,
			);
		}

		TaskStacks::Common(CommonStack {
			virt_addr,
			phys_addr,
			total_size,
		})
	}

	pub fn from_boot_stacks() -> TaskStacks {
		let stack = VirtAddr::new(CURRENT_STACK_ADDRESS.load(Ordering::Relaxed) as u64);
		debug!("Using boot stack {stack:p}");

		TaskStacks::Boot(BootStack { stack })
	}

	pub fn get_user_stack_size(&self) -> usize {
		match self {
			TaskStacks::Boot(_) => 0,
			TaskStacks::Common(stacks) => stacks.total_size - DEFAULT_STACK_SIZE,
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

#[cfg(not(target_os = "none"))]
extern "C" fn task_start(_f: extern "C" fn(usize), _arg: usize, _user_stack: u64) -> ! {
	unimplemented!()
}

#[cfg(target_os = "none")]
extern "C" fn thread_exit(status: i32) -> ! {
	debug!("Exit thread with error code {status}!");
	core_scheduler().exit(status)
}

#[cfg(target_os = "none")]
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
			stack -= mem::size_of::<State>();

			let state = stack.as_mut_ptr::<State>();
			#[cfg(not(feature = "common-os"))]
			if let Some(tls) = &self.tls {
				(*state).tpidr_el0 = tls.thread_ptr() as u64;
			}

			/*
			 * The elr_el1 needs to hold the address of the
			 * first function to be called when returning from exception handler.
			 */
			(*state).elr_el1 = task_start as *const () as usize as u64;
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
	// Trap next FPU instruction so we can lazily restore FPU state
	CPACR_EL1.modify(CPACR_EL1::FPEN::TrapEl0El1);
	isb(SY);

	core_scheduler().get_last_stack_pointer().as_u64()
}
