use align_address::Align;

use crate::arch::riscv64::kernel::core_local::core_scheduler;
use crate::arch::riscv64::mm::paging::{BasePageSize, PageSize};
use crate::scheduler::task::{Task, TaskFrame};
use crate::scheduler::{PerCoreSchedulerExt, timer_interrupts};
use crate::{DEFAULT_STACK_SIZE, KERNEL_STACK_SIZE};
use crate::mm::stack_alloc::{allocate_stack, StackAllocation};

/// For details, see [RISC-V Calling Conventions].
///
/// [RISC-V Calling Conventions]: https://github.com/riscv-non-isa/riscv-elf-psabi-doc/blob/v1.0/riscv-cc.adoc
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct State {
	/// x1: Return address
	ra: unsafe extern "C" fn(extern "C" fn(usize), usize, u64),
	/// x2: Stack pointer
	sp: usize,
	/// x3: Global pointer
	gp: usize,
	/// x4: Thread pointer
	tp: usize,
	/// x5: Temporary register
	t0: usize,
	/// x6: Temporary register
	t1: usize,
	/// x7: Temporary register
	t2: usize,
	/// x8: Callee-saved register
	s0: usize,
	/// x9: Callee-saved register
	s1: usize,
	/// x10: Argument register
	a0: usize,
	/// x11: Argument register
	a1: usize,
	/// x12: Argument register
	a2: usize,
	/// x13: Argument register
	a3: usize,
	/// x14: Argument register
	a4: usize,
	/// x15: Argument register
	a5: usize,
	/// x16: Argument register
	a6: usize,
	/// x17: Argument register
	a7: usize,
	/// x18: Callee-saved register
	s2: usize,
	/// x19: Callee-saved register
	s3: usize,
	/// x20: Callee-saved register
	s4: usize,
	/// x21: Callee-saved register
	s5: usize,
	/// x22: Callee-saved register
	s6: usize,
	/// x23: Callee-saved register
	s7: usize,
	/// x24: Callee-saved register
	s8: usize,
	/// x25: Callee-saved register
	s9: usize,
	/// x26: Callee-saved register
	s10: usize,
	/// x27: Callee-saved register
	s11: usize,
	/// x28: Temporary register
	t3: usize,
	/// x29: Temporary register
	t4: usize,
	/// x30: Temporary register
	t5: usize,
	/// x31: Temporary register
	t6: usize,
}

pub struct TaskStacks {
	kernel_stack: StackAllocation,
	user_stack: Option<StackAllocation>,
}

impl TaskStacks {
	pub fn new(size: usize) -> Self {
		let user_stack_size = if size < KERNEL_STACK_SIZE {
			KERNEL_STACK_SIZE
		} else {
			size.align_up(BasePageSize::SIZE as usize)
		};

		let kernel_stack = allocate_stack(DEFAULT_STACK_SIZE);
		let user_stack = allocate_stack(user_stack_size);

		TaskStacks {
			kernel_stack, user_stack: Some(user_stack)
		}
	}

	pub fn from_boot_stacks() -> TaskStacks {
		TaskStacks {
			kernel_stack: unsafe {
				StackAllocation::new_bootstack(KERNEL_STACK_SIZE)
			},
			user_stack: None,
		}
	}

	#[inline(always)]
	pub fn get_user_stack(&self) -> Option<&StackAllocation> {
		self.user_stack.as_ref()
	}

	#[inline(always)]
	pub fn get_kernel_stack(&self) -> &StackAllocation {
		&self.kernel_stack
	}
}

impl Clone for TaskStacks {
	fn clone(&self) -> TaskStacks {
		if let Some(user_task) = self.user_stack.as_ref() {
			TaskStacks::new(user_task.stack_size())
		} else {
			TaskStacks::from_boot_stacks()
		}
	}
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
		// Check if the task (process or thread) uses Thread-Local-Storage.
		// check is TLS is already allocated
		#[cfg(not(feature = "common-os"))]
		if self.tls.is_none() {
			use crate::scheduler::task::tls::Tls;

			self.tls = Tls::from_env();
		}

		unsafe {
			let mut stack = self.stacks.get_kernel_stack().top_of_stack();

			// Put the State structure expected by the ASM switch() function on the stack.
			stack -= size_of::<State>();

			let state = stack.as_mut_ptr::<State>();
			#[cfg(not(feature = "common-os"))]
			if let Some(tls) = &self.tls {
				(*state).tp = tls.thread_ptr().expose_provenance();
			}
			(*state).ra = task_start;
			(*state).a0 = func as usize;
			(*state).a1 = arg;

			// Set the task's stack pointer entry to the stack we have just crafted.
			self.last_stack_pointer = stack;
			self.user_stack_pointer = self.stacks.get_user_stack().unwrap().top_of_stack();

			(*state).sp = self.last_stack_pointer.as_usize();
			(*state).a2 = self.user_stack_pointer.as_usize() - size_of::<u64>();
			// trace!("state: {:#X?}", *state);
		}
	}
}

#[unsafe(naked)]
unsafe extern "C" fn task_start(func: extern "C" fn(usize), arg: usize, user_stack: u64) {
	// `func` is in the `a0` register
	// `arg` is in the `a1` register
	// `user_stack` is in the `a2` register

	core::arch::naked_asm!(
		"mv sp, a2",
		"j {task_entry}",
		task_entry = sym task_entry,
	)
}

pub fn timer_handler() {
	debug!("Handle timer interrupt");
	timer_interrupts::clear_active_and_set_next();
	core_scheduler().handle_waiting_tasks();
	core_scheduler().scheduler();
}

#[cfg(feature = "smp")]
pub fn wakeup_handler() {
	debug!("Received Wakeup Interrupt");
	//increment_irq_counter(WAKEUP_INTERRUPT_NUMBER.into());
	let core_scheduler = core_scheduler();
	core_scheduler.check_input();
	unsafe {
		riscv::register::sie::clear_ssoft();
	}
	if core_scheduler.is_scheduling() {
		core_scheduler.scheduler();
	}
}

#[inline(never)]
#[unsafe(no_mangle)]
pub fn set_current_kernel_stack() {
	core_scheduler().set_current_kernel_stack();
}
