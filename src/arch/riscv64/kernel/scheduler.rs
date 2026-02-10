use core::{mem, ptr};

use align_address::Align;
use free_list::{PageLayout, PageRange};
use memory_addresses::{PhysAddr, VirtAddr};

use crate::arch::riscv64::kernel::core_local::core_scheduler;
use crate::arch::riscv64::kernel::processor::set_oneshot_timer;
use crate::arch::riscv64::mm::paging::{BasePageSize, PageSize, PageTableEntryFlags};
use crate::mm::{FrameAlloc, PageAlloc, PageRangeAllocator};
use crate::scheduler::task::{Task, TaskFrame};
use crate::{DEFAULT_STACK_SIZE, KERNEL_STACK_SIZE};

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
		let total_size = user_stack_size + DEFAULT_STACK_SIZE + KERNEL_STACK_SIZE;
		let layout = PageLayout::from_size(total_size + 4 * BasePageSize::SIZE as usize).unwrap();
		let page_range = PageAlloc::allocate(layout).unwrap();
		let virt_addr = VirtAddr::from(page_range.start());
		let frame_layout = PageLayout::from_size(total_size).unwrap();
		let frame_range = FrameAlloc::allocate(frame_layout)
			.expect("Failed to allocate Physical Memory for TaskStacks");
		let phys_addr = PhysAddr::from(frame_range.start());

		debug!(
			"Create stacks at {:#X} with a size of {} KB",
			virt_addr,
			total_size >> 10
		);

		let mut flags = PageTableEntryFlags::empty();
		flags.normal().writable().execute_disable();

		// map IST0 into the address space
		crate::arch::mm::paging::map::<BasePageSize>(
			virt_addr + BasePageSize::SIZE,
			//virt_addr,
			phys_addr,
			KERNEL_STACK_SIZE / BasePageSize::SIZE as usize,
			flags,
		);

		// map kernel stack into the address space
		crate::arch::mm::paging::map::<BasePageSize>(
			virt_addr + KERNEL_STACK_SIZE + 2 * BasePageSize::SIZE,
			//virt_addr + KERNEL_STACK_SIZE,
			phys_addr + KERNEL_STACK_SIZE,
			DEFAULT_STACK_SIZE / BasePageSize::SIZE as usize,
			flags,
		);

		// map user stack into the address space
		crate::arch::mm::paging::map::<BasePageSize>(
			virt_addr + KERNEL_STACK_SIZE + DEFAULT_STACK_SIZE + 3 * BasePageSize::SIZE,
			//virt_addr + KERNEL_STACK_SIZE + DEFAULT_STACK_SIZE,
			phys_addr + KERNEL_STACK_SIZE + DEFAULT_STACK_SIZE,
			user_stack_size / BasePageSize::SIZE as usize,
			flags,
		);

		// clear user stack
		debug!("Clearing user stack...");
		unsafe {
			ptr::write_bytes(
				(virt_addr + KERNEL_STACK_SIZE + DEFAULT_STACK_SIZE + 3 * BasePageSize::SIZE)
					//(virt_addr + KERNEL_STACK_SIZE + DEFAULT_STACK_SIZE)
					.as_mut_ptr::<u8>(),
				0,
				user_stack_size,
			);
		}

		debug!("Creating stacks finished");

		TaskStacks::Common(CommonStack {
			virt_addr,
			phys_addr,
			total_size,
		})
	}

	pub fn from_boot_stacks() -> TaskStacks {
		TaskStacks::Boot(BootStack {
			stack: VirtAddr::zero(),
		})
	}

	pub fn get_user_stack_size(&self) -> usize {
		match self {
			TaskStacks::Boot(_) => 0,
			TaskStacks::Common(stacks) => {
				stacks.total_size - DEFAULT_STACK_SIZE - KERNEL_STACK_SIZE
			}
		}
	}

	pub fn get_user_stack(&self) -> VirtAddr {
		match self {
			TaskStacks::Boot(_) => VirtAddr::zero(),
			TaskStacks::Common(stacks) => {
				stacks.virt_addr + KERNEL_STACK_SIZE + DEFAULT_STACK_SIZE + 3 * BasePageSize::SIZE
				//stacks.virt_addr + KERNEL_STACK_SIZE + DEFAULT_STACK_SIZE
			}
		}
	}

	pub fn get_kernel_stack(&self) -> VirtAddr {
		match self {
			TaskStacks::Boot(stacks) => stacks.stack,
			TaskStacks::Common(stacks) => {
				stacks.virt_addr + KERNEL_STACK_SIZE + 2 * BasePageSize::SIZE
				//stacks.virt_addr + KERNEL_STACK_SIZE
			}
		}
	}

	pub fn get_kernel_stack_size(&self) -> usize {
		match self {
			TaskStacks::Boot(_) => KERNEL_STACK_SIZE,
			TaskStacks::Common(_) => DEFAULT_STACK_SIZE,
		}
	}
}

impl Clone for TaskStacks {
	fn clone(&self) -> TaskStacks {
		match self {
			TaskStacks::Boot(_) => TaskStacks::new(0),
			TaskStacks::Common(stacks) => {
				TaskStacks::new(stacks.total_size - DEFAULT_STACK_SIZE - KERNEL_STACK_SIZE)
			}
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
					"Deallocating stacks at {:#X} with a size of {} KB",
					stacks.virt_addr,
					stacks.total_size >> 10,
				);

				crate::arch::mm::paging::unmap::<BasePageSize>(
					stacks.virt_addr,
					stacks.total_size / BasePageSize::SIZE as usize + 4,
					//stacks.total_size / BasePageSize::SIZE as usize,
				);
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

extern "C" fn task_entry(func: extern "C" fn(usize), arg: usize) {
	use crate::scheduler::PerCoreSchedulerExt;

	// Check if the task (process or thread) uses Thread-Local-Storage.
	/*let tls_size = unsafe { &tls_end as *const u8 as usize - &tls_start as *const u8 as usize };
	if tls_size > 0 {
		// Yes, it does, so we have to allocate TLS memory.
		// Allocate enough space for the given size and one more variable of type usize, which holds the tls_pointer.
		let tls_allocation_size = tls_size + mem::size_of::<usize>();
		let tls = TaskTLS::new(tls_allocation_size);

		// The tls_pointer is the address to the end of the TLS area requested by the task.
		let tls_pointer = tls.address() + tls_size;

		// TODO: Implement AArch64 TLS

		// Associate the TLS memory to the current task.
		let mut current_task_borrowed = core_scheduler().current_task.borrow_mut();
		debug!(
			"Set up TLS for task {} at address {:#X}",
			current_task_borrowed.id,
			tls.address()
		);
		current_task_borrowed.tls = Some(tls);
	}*/

	// Call the actual entry point of the task.
	//unsafe{debug!("state: {:#X?}", *((func as usize -31*8 ) as *const crate::arch::riscv64::kernel::scheduler::State));}
	//panic!("Not impl");
	//println!("Task start");
	func(arg);
	//println!("Task end");

	// switch_to_kernel!();

	// Exit task
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
			// Set a marker for debugging at the very top.
			let mut stack =
				self.stacks.get_kernel_stack() + self.stacks.get_kernel_stack_size() - 0x10u64;
			*stack.as_mut_ptr::<u64>() = 0xdead_beefu64;

			// Put the State structure expected by the ASM switch() function on the stack.
			stack -= mem::size_of::<State>();

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
			self.user_stack_pointer =
				self.stacks.get_user_stack() + self.stacks.get_user_stack_size() - 0x10u64;

			(*state).sp = self.last_stack_pointer.as_usize();
			(*state).a2 = self.user_stack_pointer.as_usize() - mem::size_of::<u64>();
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
	//increment_irq_counter(apic::TIMER_INTERRUPT_NUMBER.into());
	core_scheduler().handle_waiting_tasks();
	set_oneshot_timer(None);
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
