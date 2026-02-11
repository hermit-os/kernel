//! Architecture dependent interface to initialize a task

use core::arch::naked_asm;
use core::mem;

use align_address::Align;
use free_list::{PageLayout, PageRange};
use memory_addresses::{PhysAddr, VirtAddr};

use super::interrupts::{IDT, IST_SIZE};
use crate::arch::x86_64::kernel::core_local::*;
use crate::arch::x86_64::kernel::{apic, interrupts};
use crate::arch::x86_64::mm::paging::{
	BasePageSize, PageSize, PageTableEntryFlags, PageTableEntryFlagsExt,
};
use crate::config::*;
use crate::env;
use crate::mm::{FrameAlloc, PageAlloc, PageRangeAllocator};
use crate::scheduler::PerCoreSchedulerExt;
use crate::scheduler::task::{Task, TaskFrame};

#[repr(C, packed)]
struct State {
	#[cfg(feature = "common-os")]
	/// GS register
	gs: u64,
	/// FS register for TLS support
	fs: u64,
	/// R15 register
	r15: u64,
	/// R14 register
	r14: u64,
	/// R13 register
	r13: u64,
	/// R12 register
	r12: u64,
	/// R11 register
	r11: u64,
	/// R10 register
	r10: u64,
	/// R9 register
	r9: u64,
	/// R8 register
	r8: u64,
	/// RDI register
	rdi: u64,
	/// RSI register
	rsi: u64,
	/// RBP register
	rbp: u64,
	/// RBX register
	rbx: u64,
	/// RDX register
	rdx: u64,
	/// RCX register
	rcx: u64,
	/// RAX register
	rax: u64,
	/// Status flags
	rflags: u64,
	/// Instruction pointer
	rip: extern "C" fn(extern "C" fn(usize), usize, u64) -> !,
}

pub struct BootStack {
	/// Stack for kernel tasks
	stack: VirtAddr,
	/// Stack to handle interrupts
	ist1: VirtAddr,
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

	pub fn new(size: usize) -> TaskStacks {
		let user_stack_size = if size < KERNEL_STACK_SIZE {
			KERNEL_STACK_SIZE
		} else {
			size.align_up(BasePageSize::SIZE as usize)
		};
		let total_size = user_stack_size + DEFAULT_STACK_SIZE + IST_SIZE;
		let layout = PageLayout::from_size(total_size + 4 * BasePageSize::SIZE as usize).unwrap();
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

		// map IST1 into the address space
		crate::arch::mm::paging::map::<BasePageSize>(
			virt_addr + BasePageSize::SIZE,
			phys_addr,
			IST_SIZE / BasePageSize::SIZE as usize,
			flags,
		);

		// map kernel stack into the address space
		crate::arch::mm::paging::map::<BasePageSize>(
			virt_addr + IST_SIZE + 2 * BasePageSize::SIZE,
			phys_addr + IST_SIZE,
			DEFAULT_STACK_SIZE / BasePageSize::SIZE as usize,
			flags,
		);

		// map user stack into the address space
		crate::arch::mm::paging::map::<BasePageSize>(
			virt_addr + IST_SIZE + DEFAULT_STACK_SIZE + 3 * BasePageSize::SIZE,
			phys_addr + IST_SIZE + DEFAULT_STACK_SIZE,
			user_stack_size / BasePageSize::SIZE as usize,
			flags,
		);

		// clear user stack
		unsafe {
			(virt_addr + IST_SIZE + DEFAULT_STACK_SIZE + 3 * BasePageSize::SIZE)
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
		let tss = unsafe { &*CoreLocal::get().tss.get() };
		let stack = VirtAddr::new(
			tss.privilege_stack_table[0].as_u64() + Self::MARKER_SIZE as u64
				- KERNEL_STACK_SIZE as u64,
		);
		debug!("Using boot stack {stack:p}");
		let ist1 = VirtAddr::new(
			tss.interrupt_stack_table[0].as_u64() + Self::MARKER_SIZE as u64 - IST_SIZE as u64,
		);
		debug!("IST1 is located at {ist1:p}");

		TaskStacks::Boot(BootStack { stack, ist1 })
	}

	pub fn get_user_stack_size(&self) -> usize {
		match self {
			TaskStacks::Boot(_) => 0,
			TaskStacks::Common(stacks) => stacks.total_size - DEFAULT_STACK_SIZE - IST_SIZE,
		}
	}

	pub fn get_user_stack(&self) -> VirtAddr {
		match self {
			TaskStacks::Boot(_) => VirtAddr::zero(),
			TaskStacks::Common(stacks) => {
				stacks.virt_addr + IST_SIZE + DEFAULT_STACK_SIZE + 3 * BasePageSize::SIZE
			}
		}
	}

	pub fn get_kernel_stack(&self) -> VirtAddr {
		match self {
			TaskStacks::Boot(stacks) => stacks.stack,
			TaskStacks::Common(stacks) => stacks.virt_addr + IST_SIZE + 2 * BasePageSize::SIZE,
		}
	}

	pub fn get_kernel_stack_size(&self) -> usize {
		match self {
			TaskStacks::Boot(_) => KERNEL_STACK_SIZE,
			TaskStacks::Common(_) => DEFAULT_STACK_SIZE,
		}
	}

	pub fn get_interrupt_stack(&self) -> VirtAddr {
		match self {
			TaskStacks::Boot(stacks) => stacks.ist1,
			TaskStacks::Common(stacks) => stacks.virt_addr + BasePageSize::SIZE,
		}
	}

	pub fn get_interrupt_stack_size(&self) -> usize {
		IST_SIZE
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

				if !env::is_uefi() {
					crate::arch::mm::paging::unmap::<BasePageSize>(
						stacks.virt_addr,
						stacks.total_size / BasePageSize::SIZE as usize + 4,
					);
				}
				let range = PageRange::from_start_len(
					stacks.virt_addr.as_usize(),
					stacks.total_size + 4 * BasePageSize::SIZE as usize,
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

#[unsafe(naked)]
extern "C" fn task_start(_f: extern "C" fn(usize), _arg: usize, _user_stack: u64) -> ! {
	// `f` is in the `rdi` register
	// `arg` is in the `rsi` register
	// `user_stack` is in the `rdx` register

	naked_asm!(
		"mov rsp, rdx",
		"sti",
		"jmp {task_entry}",
		task_entry = sym task_entry,
	)
}

extern "C" fn task_entry(func: extern "C" fn(usize), arg: usize) -> ! {
	// Call the actual entry point of the task.
	func(arg);

	// Exit task
	debug!("Exit thread with error code 0!");
	core_scheduler().exit(0)
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
				(*state).fs = tls.thread_ptr().addr() as u64;
			}
			(*state).rip = task_start;
			(*state).rdi = func as usize as u64;
			(*state).rsi = arg as u64;

			// per default we disable interrupts
			(*state).rflags = 0x1202u64;

			// Set the task's stack pointer entry to the stack we have just crafted.
			self.last_stack_pointer = stack;
			self.user_stack_pointer = self.stacks.get_user_stack()
				+ self.stacks.get_user_stack_size()
				- TaskStacks::MARKER_SIZE;

			// rdx is required to initialize the stack
			(*state).rdx = self.user_stack_pointer.as_u64() - mem::size_of::<u64>() as u64;
		}
	}
}

extern "x86-interrupt" fn timer_handler(_stack_frame: interrupts::ExceptionStackFrame) {
	increment_irq_counter(apic::TIMER_INTERRUPT_NUMBER);
	core_scheduler().handle_waiting_tasks();
	apic::eoi();
	core_scheduler().reschedule();
}

pub fn install_timer_handler() {
	unsafe {
		let mut idt = IDT.lock();
		idt[apic::TIMER_INTERRUPT_NUMBER]
			.set_handler_fn(timer_handler)
			.set_stack_index(0);
	}
	interrupts::add_irq_name(apic::TIMER_INTERRUPT_NUMBER - 32, "Timer");
}
