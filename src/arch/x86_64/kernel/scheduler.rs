//! Architecture dependent interface to initialize a task

use core::arch::naked_asm;
use core::mem::MaybeUninit;
use super::interrupts::{IDT, IST_SIZE};
use crate::arch::interrupts::IST_ENTRIES;
use crate::arch::x86_64::kernel::core_local::*;
use crate::arch::x86_64::kernel::{apic, interrupts};
use crate::arch::x86_64::mm::paging::{BasePageSize, PageSize};
use crate::config::*;
use crate::mm::stack_alloc::{allocate_stack, StackAllocation};
use crate::scheduler::task::{Task, TaskFrame};
use crate::scheduler::{timer_interrupts, PerCoreSchedulerExt};
use align_address::Align;

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

pub struct TaskStacks {
	ist_stacks: [StackAllocation; IST_ENTRIES],
	kernel_stack: StackAllocation,
	user_stack: Option<StackAllocation>,
}

impl TaskStacks {
	pub fn new(size: usize) -> TaskStacks {
		let user_stack_size = if size < KERNEL_STACK_SIZE {
			KERNEL_STACK_SIZE
		} else {
			size.align_up(BasePageSize::SIZE as usize)
		};

		// map IST1 into the address space
		let mut ist_stacks = [const { MaybeUninit::<StackAllocation>::uninit() }; IST_ENTRIES];
		#[allow(clippy::needless_range_loop)]
		for i in 0..IST_ENTRIES {
			let size = if i == 0 {
				IST_SIZE
			} else {
				BasePageSize::SIZE as usize
			};

			let stack = allocate_stack(size);
			ist_stacks[i] = MaybeUninit::new(stack);
		}
		let ist_stacks: MaybeUninit<[StackAllocation; 4]> = ist_stacks.into();

		let kernel_stack = allocate_stack(DEFAULT_STACK_SIZE);
		let user_stack = allocate_stack(user_stack_size);

		TaskStacks {
			ist_stacks: unsafe { ist_stacks.assume_init() }, kernel_stack, user_stack: Some(user_stack)
		}
	}

	pub fn from_boot_stacks() -> TaskStacks {
		let core_local = CoreLocal::get();
		let kernel = core_local.kernel_stack.borrow().as_ref().expect("no kernel stack").weak();

		let mut ist_stacks = [const { MaybeUninit::<StackAllocation>::uninit() }; IST_ENTRIES];
		for (i, stack) in core_local.interrupt_stack_allocs.iter().enumerate() {
			ist_stacks[i]  = MaybeUninit::new(stack.borrow().as_ref().expect("no ist stack").weak());
		}
		let ist_stacks: MaybeUninit<[StackAllocation; 4]> = ist_stacks.into();

		TaskStacks {
			ist_stacks: unsafe { ist_stacks.assume_init() }, kernel_stack: kernel, user_stack: None
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

	#[inline(always)]
	pub fn get_interrupt_stacks(&self) -> &[StackAllocation; IST_ENTRIES] {
		&self.ist_stacks
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
			let mut stack = self.stacks.get_kernel_stack().top_of_stack();

			// Put the State structure expected by the ASM switch() function on the stack.
			stack -= size_of::<State>();

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
			self.user_stack_pointer = self.stacks.get_user_stack().unwrap().top_of_stack();

			// rdx is required to initialize the stack
			(*state).rdx = self.user_stack_pointer.as_u64() - size_of::<u64>() as u64;
		}
	}
}

extern "x86-interrupt" fn timer_handler(_stack_frame: interrupts::ExceptionStackFrame) {
	increment_irq_counter(apic::TIMER_INTERRUPT_NUMBER);

	debug!("Handle timer interrupt");
	timer_interrupts::clear_active_and_set_next();

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
