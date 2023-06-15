//! Architecture dependent interface to initialize a task

use alloc::alloc::{alloc_zeroed, Layout};
use alloc::boxed::Box;
use core::arch::asm;
use core::sync::atomic::Ordering;
use core::{mem, ptr, slice};

use align_address::Align;

use crate::arch::aarch64::kernel::core_local::core_scheduler;
use crate::arch::aarch64::kernel::CURRENT_STACK_ADDRESS;
use crate::arch::aarch64::mm::paging::{BasePageSize, PageSize, PageTableEntryFlags};
use crate::arch::aarch64::mm::{PhysAddr, VirtAddr};
use crate::scheduler::task::{Task, TaskFrame};
use crate::{env, DEFAULT_STACK_SIZE, KERNEL_STACK_SIZE};

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
		let virt_addr =
			crate::arch::mm::virtualmem::allocate(total_size + 3 * BasePageSize::SIZE as usize)
				.expect("Failed to allocate Virtual Memory for TaskStacks");
		let phys_addr = crate::arch::mm::physicalmem::allocate(total_size)
			.expect("Failed to allocate Physical Memory for TaskStacks");

		debug!(
			"Create stacks at {:#X} with a size of {} KB",
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
				(virt_addr + DEFAULT_STACK_SIZE + 2 * BasePageSize::SIZE as usize)
					.as_mut_ptr::<u8>(),
				0xAC,
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
		let stack = VirtAddr::from_u64(CURRENT_STACK_ADDRESS.load(Ordering::Relaxed));
		debug!("Using boot stack {:#X}", stack);

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
				stacks.virt_addr + DEFAULT_STACK_SIZE + 2 * BasePageSize::SIZE as usize
			}
		}
	}

	pub fn get_kernel_stack(&self) -> VirtAddr {
		match self {
			TaskStacks::Boot(stacks) => stacks.stack,
			TaskStacks::Common(stacks) => stacks.virt_addr + BasePageSize::SIZE as usize,
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
					"Deallocating stacks at {:#X} with a size of {} KB",
					stacks.virt_addr,
					stacks.total_size >> 10,
				);

				crate::arch::mm::paging::unmap::<BasePageSize>(
					stacks.virt_addr,
					stacks.total_size / BasePageSize::SIZE as usize + 3,
				);
				crate::arch::mm::virtualmem::deallocate(
					stacks.virt_addr,
					stacks.total_size + 3 * BasePageSize::SIZE as usize,
				);
				crate::arch::mm::physicalmem::deallocate(stacks.phys_addr, stacks.total_size);
			}
		}
	}
}

/*
 * https://fuchsia.dev/fuchsia-src/development/kernel/threads/tls and
 * and https://uclibc.org/docs/tls.pdf is used to understand variant 1
 * of the TLS implementations.
 */

#[derive(Copy, Clone)]
#[repr(C)]
struct DtvPointer {
	val: *const (),
	to_free: *const (),
}

#[repr(C)]
union Dtv {
	counter: usize,
	pointer: DtvPointer,
}

#[repr(C)]
pub struct TaskTLS {
	dtv: mem::MaybeUninit<Box<[Dtv; 2]>>,
	_private: usize,
	block: [u8],
}

impl TaskTLS {
	fn from_environment() -> Option<Box<Self>> {
		if env::get_tls_memsz() == 0 {
			return None;
		}

		// Get TLS initialization image
		let tls_init_image = {
			let tls_init_data = env::get_tls_start().as_ptr::<u8>();
			let tls_init_len = env::get_tls_filesz();

			// SAFETY: We will have to trust the environment here.
			unsafe { slice::from_raw_parts(tls_init_data, tls_init_len) }
		};

		let off = core::cmp::max(16, env::get_tls_align()) - 16;
		let block_len = env::get_tls_memsz() + off;
		let len = block_len + mem::size_of::<Box<[Dtv; 2]>>();

		let layout = Layout::from_size_align(len, 16).unwrap();
		let mut this = unsafe {
			let data = alloc_zeroed(layout);
			let raw = ptr::slice_from_raw_parts_mut(data, block_len) as *mut TaskTLS;

			let addr = (*raw).block.as_ptr().offset(off as isize).cast::<()>();
			(*raw).dtv.as_mut_ptr().write(Box::new([
				Dtv { counter: 1 },
				Dtv {
					pointer: DtvPointer {
						val: addr,
						to_free: ptr::null(),
					},
				},
			]));

			Box::from_raw(raw)
		};

		this.block[off..off + tls_init_image.len()].copy_from_slice(tls_init_image);

		Some(this)
	}

	fn thread_ptr(&self) -> *const Box<[Dtv; 2]> {
		self.dtv.as_ptr()
	}
}

#[cfg(not(target_os = "none"))]
extern "C" fn task_start(_f: extern "C" fn(usize), _arg: usize, _user_stack: u64) -> ! {
	unimplemented!()
}

#[cfg(target_os = "none")]
#[naked]
extern "C" fn task_start(_f: extern "C" fn(usize), _arg: usize) -> ! {
	// `f` is in the `x0` register
	// `arg` is in the `x1` register

	unsafe {
		asm!(
			"msr spsel, {l0}",
			"mov x25, x0",
			"mov x0, x1",
			"blr x25",
			"mov x0, xzr",
			"adrp x4, {exit}",
			"add  x4, x4, #:lo12:{exit}",
			"br x4",
			l0 = const 0,
			exit = sym crate::sys_thread_exit,
			options(noreturn)
		)
	}
}

impl TaskFrame for Task {
	fn create_stack_frame(&mut self, func: extern "C" fn(usize), arg: usize) {
		// Check if TLS is allocated already and if the task uses thread-local storage.
		if self.tls.is_none() {
			self.tls = TaskTLS::from_environment();
		}

		unsafe {
			// Set a marker for debugging at the very top.
			let mut stack = self.stacks.get_kernel_stack() + self.stacks.get_kernel_stack_size()
				- TaskStacks::MARKER_SIZE;
			*stack.as_mut_ptr::<u64>() = 0xDEAD_BEEFu64;

			// Put the State structure expected by the ASM switch() function on the stack.
			stack = stack - mem::size_of::<State>();

			let state = stack.as_mut_ptr::<State>();
			ptr::write_bytes(stack.as_mut_ptr::<u8>(), 0, mem::size_of::<State>());

			if let Some(tls) = &self.tls {
				(*state).tpidr_el0 = tls.thread_ptr() as u64;
			}

			/*
			 * The elr_el1 needs to hold the address of the
			 * first function to be called when returning from exception handler.
			 */
			(*state).elr_el1 = task_start as usize as u64;
			(*state).x0 = func as usize as u64; // use second argument to transfer the entry point
			(*state).x1 = arg as u64;
			(*state).spsel = 1;

			/* Zero the condition flags. */
			(*state).spsr_el1 = 0x3E5;

			// Set the task's stack pointer entry to the stack we have just crafted.
			self.last_stack_pointer = stack;

			// initialize user-level stack
			self.user_stack_pointer = self.stacks.get_user_stack()
				+ self.stacks.get_user_stack_size()
				- TaskStacks::MARKER_SIZE;
			*self.user_stack_pointer.as_mut_ptr::<u64>() = 0xDEAD_BEEFu64;
			(*state).sp_el0 = self.user_stack_pointer.as_u64();
		}
	}
}

#[no_mangle]
pub(crate) extern "C" fn get_last_stack_pointer() -> u64 {
	core_scheduler().get_last_stack_pointer().as_u64()
}
