use core::{mem, ptr};

use align_address::Align;
use free_list::{PageLayout, PageRange};
use memory_addresses::{PhysAddr, VirtAddr};

use crate::arch::riscv64::kernel::core_local::core_scheduler;
use crate::arch::riscv64::kernel::processor::set_oneshot_timer;
use crate::arch::riscv64::mm::paging::{BasePageSize, PageSize, PageTableEntryFlags};
use crate::mm::physicalmem::{allocate_physical, deallocate_physical};
use crate::mm::virtualmem::KERNEL_FREE_LIST;
use crate::scheduler::task::{Task, TaskFrame};
use crate::{DEFAULT_STACK_SIZE, KERNEL_STACK_SIZE};

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct State {
	/// return address register
	ra: usize,
	/// stack pointer register
	sp: usize,
	/// global pointer register
	gp: usize,
	/// thread pointer register
	tp: usize,
	/// x5 register
	x5: usize,
	/// x6 register
	x6: usize,
	/// x7 register
	x7: usize,
	/// x8 register
	x8: usize,
	/// x9 register
	x9: usize,
	/// Function arguments/return values
	a0: usize,
	/// a1 register
	a1: usize,
	/// a2 register
	a2: usize,
	/// x13 register
	x13: usize,
	/// x14 register
	x14: usize,
	/// x15 register
	x15: usize,
	/// x16 register
	x16: usize,
	/// x17 register
	x17: usize,
	/// x18 register
	x18: usize,
	/// x19 register
	x19: usize,
	/// x20 register
	x20: usize,
	/// x21 register
	x21: usize,
	/// x22 register
	x22: usize,
	/// x23 register
	x23: usize,
	/// x24 register
	x24: usize,
	/// x25 register
	x25: usize,
	/// x26 register
	x26: usize,
	/// x27 register
	x27: usize,
	/// x28 register
	x28: usize,
	/// x29 register
	x29: usize,
	/// x30 register
	x30: usize,
	/// x31 register
	x31: usize,
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
		let total_size = user_stack_size + DEFAULT_STACK_SIZE + KERNEL_STACK_SIZE;
		let layout = PageLayout::from_size(total_size + 4 * BasePageSize::SIZE as usize).unwrap();
		let page_range = KERNEL_FREE_LIST.lock().allocate(layout).unwrap();
		let virt_addr = VirtAddr::from(page_range.start());
		let phys_addr = allocate_physical(total_size, free_list::PAGE_SIZE)
			.expect("Failed to allocate Physical Memory for TaskStacks");

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
					KERNEL_FREE_LIST.lock().deallocate(range).unwrap();
				}

				let range =
					PageRange::from_start_len(stacks.phys_addr.as_usize(), stacks.total_size)
						.unwrap();
				unsafe {
					deallocate_physical(PhysAddr::new(range.start() as u64), range.len().get());
				}
			}
		}
	}
}

#[unsafe(no_mangle)]
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
				(*state).tp = tls.thread_ptr() as usize;
			}
			(*state).ra = task_start as usize;
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

unsafe extern "C" {
	fn task_start(func: extern "C" fn(usize), arg: usize, user_stack: u64);
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
