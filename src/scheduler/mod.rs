use alloc::boxed::Box;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::rc::Rc;
#[cfg(feature = "smp")]
use alloc::vec::Vec;
use core::cell::RefCell;
#[cfg(feature = "tcp")]
use core::ops::DerefMut;
use core::sync::atomic::{AtomicU32, Ordering};

use crossbeam_utils::Backoff;
use hermit_sync::{without_interrupts, *};

use crate::arch;
use crate::arch::core_local::*;
use crate::arch::interrupts;
#[cfg(target_arch = "x86_64")]
use crate::arch::switch::{switch_to_fpu_owner, switch_to_task};
use crate::kernel::scheduler::TaskStacks;
use crate::scheduler::task::*;
pub mod task;

static NO_TASKS: AtomicU32 = AtomicU32::new(0);
/// Map between Core ID and per-core scheduler
#[cfg(feature = "smp")]
static SCHEDULER_INPUTS: SpinMutex<Vec<&InterruptTicketMutex<SchedulerInput>>> =
	SpinMutex::new(Vec::new());
/// Map between Task ID and Queue of waiting tasks
static WAITING_TASKS: InterruptTicketMutex<BTreeMap<TaskId, VecDeque<TaskHandle>>> =
	InterruptTicketMutex::new(BTreeMap::new());
/// Map between Task ID and TaskHandle
static TASKS: InterruptTicketMutex<BTreeMap<TaskId, TaskHandle>> =
	InterruptTicketMutex::new(BTreeMap::new());

/// Unique identifier for a core.
pub type CoreId = u32;

#[cfg(feature = "smp")]
struct SchedulerInput {
	/// Queue of new tasks
	new_tasks: VecDeque<NewTask>,
	/// Queue of task, which are wakeup by another core
	wakeup_tasks: VecDeque<TaskHandle>,
}

#[cfg(feature = "smp")]
impl SchedulerInput {
	pub fn new() -> Self {
		Self {
			new_tasks: VecDeque::new(),
			wakeup_tasks: VecDeque::new(),
		}
	}
}

#[cfg_attr(any(target_arch = "x86_64", target_arch = "aarch64"), repr(align(128)))]
#[cfg_attr(
	not(any(target_arch = "x86_64", target_arch = "aarch64")),
	repr(align(64))
)]
pub struct PerCoreScheduler {
	/// Core ID of this per-core scheduler
	#[cfg(feature = "smp")]
	core_id: CoreId,
	/// Task which is currently running
	current_task: Rc<RefCell<Task>>,
	/// Idle Task
	idle_task: Rc<RefCell<Task>>,
	/// Task that currently owns the FPU
	#[cfg(target_arch = "x86_64")]
	fpu_owner: Rc<RefCell<Task>>,
	/// Queue of tasks, which are ready
	ready_queue: PriorityTaskQueue,
	/// Queue of tasks, which are finished and can be released
	finished_tasks: VecDeque<Rc<RefCell<Task>>>,
	/// Queue of blocked tasks, sorted by wakeup time.
	blocked_tasks: BlockedTaskQueue,
	/// Queue of blocked tasks, sorted by wakeup time.
	#[cfg(feature = "tcp")]
	blocked_async_tasks: VecDeque<TaskHandle>,
	/// Queues to handle incoming requests from the other cores
	#[cfg(feature = "smp")]
	input: InterruptTicketMutex<SchedulerInput>,
}

struct NewTask {
	tid: TaskId,
	func: extern "C" fn(usize),
	arg: usize,
	prio: Priority,
	core_id: CoreId,
	stacks: TaskStacks,
}

impl From<NewTask> for Task {
	fn from(value: NewTask) -> Self {
		let NewTask {
			tid,
			func,
			arg,
			prio,
			core_id,
			stacks,
		} = value;
		let mut task = Self::new(tid, core_id, TaskStatus::Ready, prio, stacks);
		task.create_stack_frame(func, arg);
		task
	}
}

impl PerCoreScheduler {
	/// Spawn a new task.
	pub fn spawn(
		func: extern "C" fn(usize),
		arg: usize,
		prio: Priority,
		core_id: CoreId,
		stack_size: usize,
	) -> TaskId {
		// Create the new task.
		let tid = get_tid();
		let stacks = TaskStacks::new(stack_size);
		let new_task = NewTask {
			tid,
			func,
			arg,
			prio,
			core_id,
			stacks,
		};

		// Add it to the task lists.
		let wakeup = {
			#[cfg(feature = "smp")]
			let mut input_locked = get_scheduler_input(core_id).lock();
			WAITING_TASKS.lock().insert(tid, VecDeque::with_capacity(1));
			TASKS.lock().insert(
				tid,
				TaskHandle::new(
					tid,
					prio,
					#[cfg(feature = "smp")]
					core_id,
				),
			);
			NO_TASKS.fetch_add(1, Ordering::SeqCst);

			#[cfg(feature = "smp")]
			if core_id != core_scheduler().core_id {
				input_locked.new_tasks.push_back(new_task);
				true
			} else {
				let task = Rc::new(RefCell::new(Task::from(new_task)));
				core_scheduler().ready_queue.push(task);
				false
			}
			#[cfg(not(feature = "smp"))]
			if core_id == 0 {
				let task = Rc::new(RefCell::new(Task::from(new_task)));
				core_scheduler().ready_queue.push(task);
				false
			} else {
				panic!("Invalid  core_id {}!", core_id)
			}
		};

		debug!(
			"Creating task {} with priority {} on core {}",
			tid, prio, core_id
		);

		if wakeup {
			arch::wakeup_core(core_id);
		}

		tid
	}

	/// Terminate the current task on the current core.
	pub fn exit(&mut self, exit_code: i32) -> ! {
		let closure = || {
			// Get the current task.
			let mut current_task_borrowed = self.current_task.borrow_mut();
			assert_ne!(
				current_task_borrowed.status,
				TaskStatus::Idle,
				"Trying to terminate the idle task"
			);

			// Finish the task and reschedule.
			debug!(
				"Finishing task {} with exit code {}",
				current_task_borrowed.id, exit_code
			);
			current_task_borrowed.status = TaskStatus::Finished;
			NO_TASKS.fetch_sub(1, Ordering::SeqCst);

			let current_id = current_task_borrowed.id;
			drop(current_task_borrowed);

			// wakeup tasks, which are waiting for task with the identifier id
			if let Some(mut queue) = WAITING_TASKS.lock().remove(&current_id) {
				while let Some(task) = queue.pop_front() {
					self.custom_wakeup(task);
				}
			}
		};

		without_interrupts(closure);

		self.reschedule();
		unreachable!()
	}

	#[cfg(feature = "newlib")]
	fn clone_impl(&self, func: extern "C" fn(usize), arg: usize) -> TaskId {
		static NEXT_CORE_ID: AtomicU32 = AtomicU32::new(1);

		// Get the Core ID of the next CPU.
		let core_id: CoreId = {
			// Increase the CPU number by 1.
			let id = NEXT_CORE_ID.fetch_add(1, Ordering::SeqCst);

			// Check for overflow.
			if id == arch::get_processor_count() {
				NEXT_CORE_ID.store(0, Ordering::SeqCst);
				0
			} else {
				id
			}
		};

		// Get the current task.
		let current_task_borrowed = self.current_task.borrow();

		// Clone the current task.
		let tid = get_tid();
		let clone_task = NewTask {
			tid,
			func,
			arg,
			prio: current_task_borrowed.prio,
			core_id,
			stacks: TaskStacks::new(current_task_borrowed.stacks.get_user_stack_size()),
		};

		// Add it to the task lists.
		let wakeup = {
			#[cfg(feature = "smp")]
			let mut input_locked = get_scheduler_input(core_id).lock();
			WAITING_TASKS.lock().insert(tid, VecDeque::with_capacity(1));
			TASKS.lock().insert(
				tid,
				TaskHandle::new(
					tid,
					current_task_borrowed.prio,
					#[cfg(feature = "smp")]
					core_id,
				),
			);
			NO_TASKS.fetch_add(1, Ordering::SeqCst);
			#[cfg(feature = "smp")]
			if core_id != core_scheduler().core_id {
				input_locked.new_tasks.push_back(clone_task);
				true
			} else {
				let clone_task = Rc::new(RefCell::new(Task::from(clone_task)));
				core_scheduler().ready_queue.push(clone_task);
				false
			}
			#[cfg(not(feature = "smp"))]
			if core_id == 0 {
				let clone_task = Rc::new(RefCell::new(Task::from(clone_task)));
				core_scheduler().ready_queue.push(clone_task);
				false
			} else {
				panic!("Invalid core_id {}!", core_id);
			}
		};

		// Wake up the CPU
		if wakeup {
			arch::wakeup_core(core_id);
		}

		tid
	}

	#[cfg(feature = "newlib")]
	pub fn clone(&self, func: extern "C" fn(usize), arg: usize) -> TaskId {
		without_interrupts(|| self.clone_impl(func, arg))
	}

	/// Returns `true` if a reschedule is required
	#[inline]
	#[cfg(feature = "smp")]
	pub fn is_scheduling(&self) -> bool {
		self.current_task.borrow().prio < self.ready_queue.get_highest_priority()
	}

	#[inline]
	pub fn handle_waiting_tasks(&mut self) {
		without_interrupts(|| {
			#[cfg(feature = "tcp")]
			self.wakeup_async_tasks();
			self.blocked_tasks.handle_waiting_tasks()
		});
	}

	#[cfg(not(feature = "smp"))]
	pub fn custom_wakeup(&mut self, task: TaskHandle) {
		without_interrupts(|| self.blocked_tasks.custom_wakeup(task));
	}

	#[cfg(feature = "smp")]
	pub fn custom_wakeup(&mut self, task: TaskHandle) {
		if task.get_core_id() == self.core_id {
			without_interrupts(|| self.blocked_tasks.custom_wakeup(task));
		} else {
			get_scheduler_input(task.get_core_id())
				.lock()
				.wakeup_tasks
				.push_back(task);
			// Wake up the CPU
			arch::wakeup_core(task.get_core_id());
		}
	}

	#[inline]
	pub fn block_current_task(&mut self, wakeup_time: Option<u64>) {
		without_interrupts(|| {
			self.blocked_tasks
				.add(self.current_task.clone(), wakeup_time)
		});
	}

	#[cfg(feature = "tcp")]
	#[inline]
	pub fn add_network_timer(&mut self, wakeup_time: Option<u64>) {
		without_interrupts(|| self.blocked_tasks.add_network_timer(wakeup_time))
	}

	#[cfg(feature = "tcp")]
	#[inline]
	pub fn block_current_async_task(&mut self) {
		without_interrupts(|| {
			self.blocked_async_tasks
				.push_back(self.get_current_task_handle());
			self.blocked_tasks.add(self.current_task.clone(), None)
		});
	}

	#[cfg(feature = "tcp")]
	#[inline]
	pub fn wakeup_async_tasks(&mut self) {
		let mut has_tasks = false;

		without_interrupts(|| {
			while let Some(task) = self.blocked_async_tasks.pop_front() {
				has_tasks = true;
				self.custom_wakeup(task)
			}

			if !has_tasks {
				if let Some(mut guard) = crate::executor::NIC.try_lock() {
					if let crate::executor::NetworkState::Initialized(nic) = guard.deref_mut() {
						let time = crate::executor::now();
						nic.poll_common(time);
						let wakeup_time = nic
							.poll_delay(time)
							.map(|d| crate::arch::processor::get_timer_ticks() + d.total_micros());
						self.add_network_timer(wakeup_time);
					}
				}
			}
		});
	}

	#[inline]
	pub fn get_current_task_handle(&self) -> TaskHandle {
		without_interrupts(|| {
			let current_task_borrowed = self.current_task.borrow();

			TaskHandle::new(
				current_task_borrowed.id,
				current_task_borrowed.prio,
				#[cfg(feature = "smp")]
				current_task_borrowed.core_id,
			)
		})
	}

	#[cfg(feature = "newlib")]
	#[inline]
	pub fn set_lwip_errno(&self, errno: i32) {
		without_interrupts(|| self.current_task.borrow_mut().lwip_errno = errno);
	}

	#[cfg(feature = "newlib")]
	#[inline]
	pub fn get_lwip_errno(&self) -> i32 {
		without_interrupts(|| self.current_task.borrow().lwip_errno)
	}

	#[inline]
	pub fn get_current_task_id(&self) -> TaskId {
		without_interrupts(|| self.current_task.borrow().id)
	}

	#[inline]
	pub fn get_current_task_prio(&self) -> Priority {
		without_interrupts(|| self.current_task.borrow().prio)
	}

	#[cfg(target_arch = "x86_64")]
	pub fn set_current_kernel_stack(&self) {
		use x86_64::VirtAddr;

		let current_task_borrowed = self.current_task.borrow();
		let tss = unsafe { &mut *CoreLocal::get().tss.get() };

		let rsp = (current_task_borrowed.stacks.get_kernel_stack()
			+ current_task_borrowed.stacks.get_kernel_stack_size()
			- TaskStacks::MARKER_SIZE)
			.as_u64();
		tss.privilege_stack_table[0] = VirtAddr::new(rsp);
		CoreLocal::get().kernel_stack.set(rsp);
		let ist_start = (current_task_borrowed.stacks.get_interrupt_stack()
			+ current_task_borrowed.stacks.get_interrupt_stack_size()
			- TaskStacks::MARKER_SIZE)
			.as_u64();
		tss.interrupt_stack_table[0] = VirtAddr::new(ist_start);
	}

	pub fn set_current_task_priority(&mut self, prio: Priority) {
		without_interrupts(|| {
			trace!("Change priority of the current task");
			self.current_task.borrow_mut().prio = prio;
		});
	}

	pub fn set_priority(&mut self, id: TaskId, prio: Priority) -> Result<(), ()> {
		trace!("Change priority of task {} to priority {}", id, prio);

		without_interrupts(|| {
			let task = get_task_handle(id).ok_or(())?;
			#[cfg(feature = "smp")]
			let other_core = task.get_core_id() != self.core_id;
			#[cfg(not(feature = "smp"))]
			let other_core = false;

			if other_core {
				warn!("Have to change the priority on another core");
			} else if self.current_task.borrow().id == task.get_id() {
				self.current_task.borrow_mut().prio = prio;
			} else {
				self.ready_queue
					.set_priority(task, prio)
					.expect("Do not find valid task in ready queue");
			}

			Ok(())
		})
	}

	/// Save the FPU context for the current FPU owner and restore it for the current task,
	/// which wants to use the FPU now.
	#[cfg(target_arch = "x86_64")]
	pub fn fpu_switch(&mut self) {
		if !Rc::ptr_eq(&self.current_task, &self.fpu_owner) {
			debug!(
				"Switching FPU owner from task {} to {}",
				self.fpu_owner.borrow().id,
				self.current_task.borrow().id
			);

			self.fpu_owner.borrow_mut().last_fpu_state.save();
			self.current_task.borrow().last_fpu_state.restore();
			self.fpu_owner = self.current_task.clone();
		}
	}

	/// Check if a finished task could be deleted.
	fn cleanup_tasks(&mut self) {
		// Pop the first finished task and remove it from the TASKS list, which implicitly deallocates all associated memory.
		while let Some(finished_task) = self.finished_tasks.pop_front() {
			debug!("Cleaning up task {}", finished_task.borrow().id);
		}
	}

	#[cfg(feature = "smp")]
	pub fn check_input(&mut self) {
		let mut input_locked = self.input.lock();

		while let Some(task) = input_locked.wakeup_tasks.pop_front() {
			self.blocked_tasks.custom_wakeup(task);
		}

		while let Some(new_task) = input_locked.new_tasks.pop_front() {
			let task = Rc::new(RefCell::new(Task::from(new_task)));
			self.ready_queue.push(task.clone());
		}
	}

	/// Triggers the scheduler to reschedule the tasks.
	/// Interrupt flag will be cleared during the reschedule
	#[cfg(target_arch = "x86_64")]
	pub fn reschedule(&mut self) {
		without_interrupts(|| {
			if let Some(last_stack_pointer) = self.scheduler() {
				let (new_stack_pointer, is_idle) = {
					let borrowed = self.current_task.borrow();
					(
						borrowed.last_stack_pointer,
						borrowed.status == TaskStatus::Idle,
					)
				};

				if is_idle || Rc::ptr_eq(&self.current_task, &self.fpu_owner) {
					unsafe {
						switch_to_fpu_owner(last_stack_pointer, new_stack_pointer.as_usize());
					}
				} else {
					unsafe {
						switch_to_task(last_stack_pointer, new_stack_pointer.as_usize());
					}
				}
			}
		})
	}

	/// Trigger an interrupt to reschedule the system
	#[cfg(target_arch = "aarch64")]
	pub fn reschedule(&self) {
		use core::arch::asm;

		use arm_gic::gicv3::{GicV3, IntId, SgiTarget};

		use crate::interrupts::SGI_RESCHED;

		unsafe {
			asm!("dsb nsh", "isb", options(nostack, nomem, preserves_flags));
		}

		let reschedid = IntId::sgi(SGI_RESCHED.into());
		GicV3::send_sgi(
			reschedid,
			SgiTarget::List {
				affinity3: 0,
				affinity2: 0,
				affinity1: 0,
				target_list: 0b1,
			},
		);
	}

	/// Only the idle task should call this function.
	/// Set the idle task to halt state if not another
	/// available.
	pub fn run(&mut self) -> ! {
		let backoff = Backoff::new();

		loop {
			interrupts::disable();
			// do housekeeping
			self.cleanup_tasks();

			if self.ready_queue.is_empty() {
				if backoff.is_completed() {
					interrupts::enable_and_wait();
				} else {
					interrupts::enable();
					backoff.snooze();
				}
			} else {
				interrupts::enable();
				self.reschedule();
				backoff.reset();
			}
		}
	}

	#[inline]
	#[cfg(target_arch = "aarch64")]
	pub fn get_last_stack_pointer(&self) -> crate::arch::mm::VirtAddr {
		self.current_task.borrow().last_stack_pointer
	}

	/// Triggers the scheduler to reschedule the tasks.
	/// Interrupt flag must be cleared before calling this function.
	pub fn scheduler(&mut self) -> Option<*mut usize> {
		// Someone wants to give up the CPU
		// => we have time to cleanup the system
		self.cleanup_tasks();

		// Get information about the current task.
		let (id, last_stack_pointer, prio, status) = {
			let mut borrowed = self.current_task.borrow_mut();
			(
				borrowed.id,
				&mut borrowed.last_stack_pointer as *mut _ as *mut usize,
				borrowed.prio,
				borrowed.status,
			)
		};

		let mut new_task = None;

		if status == TaskStatus::Running {
			// A task is currently running.
			// Check if a task with a equal or higher priority is available.
			if let Some(task) = self.ready_queue.pop_with_prio(prio) {
				new_task = Some(task);
			}
		} else {
			if status == TaskStatus::Finished {
				// Mark the finished task as invalid and add it to the finished tasks for a later cleanup.
				self.current_task.borrow_mut().status = TaskStatus::Invalid;
				self.finished_tasks.push_back(self.current_task.clone());
			}

			// No task is currently running.
			// Check if there is any available task and get the one with the highest priority.
			if let Some(task) = self.ready_queue.pop() {
				// This available task becomes the new task.
				debug!("Task is available.");
				new_task = Some(task);
			} else if status != TaskStatus::Idle {
				// The Idle task becomes the new task.
				debug!("Only Idle Task is available.");
				new_task = Some(self.idle_task.clone());
			}
		}

		if let Some(task) = new_task {
			// There is a new task we want to switch to.

			// Handle the current task.
			if status == TaskStatus::Running {
				// Mark the running task as ready again and add it back to the queue.
				self.current_task.borrow_mut().status = TaskStatus::Ready;
				self.ready_queue.push(self.current_task.clone());
			}

			// Handle the new task and get information about it.
			let (new_id, new_stack_pointer) = {
				let mut borrowed = task.borrow_mut();
				if borrowed.status != TaskStatus::Idle {
					// Mark the new task as running.
					borrowed.status = TaskStatus::Running;
				}

				(borrowed.id, borrowed.last_stack_pointer)
			};

			if id != new_id {
				// Tell the scheduler about the new task.
				debug!(
					"Switching task from {} to {} (stack {:#X} => {:#X})",
					id,
					new_id,
					unsafe { *last_stack_pointer },
					new_stack_pointer
				);
				self.current_task = task;

				// Finally return the context of the new task.
				return Some(last_stack_pointer);
			}
		}

		None
	}
}

fn get_tid() -> TaskId {
	static TID_COUNTER: AtomicU32 = AtomicU32::new(0);
	let guard = TASKS.lock();

	loop {
		let id = TaskId::from(TID_COUNTER.fetch_add(1, Ordering::SeqCst));
		if !guard.contains_key(&id) {
			return id;
		}
	}
}

#[inline]
pub fn abort() -> ! {
	core_scheduler().exit(-1)
}

/// Add a per-core scheduler for the current core.
pub fn add_current_core() {
	// Create an idle task for this core.
	let core_id = core_id();
	let tid = get_tid();
	let idle_task = Rc::new(RefCell::new(Task::new_idle(tid, core_id)));

	// Add the ID -> Task mapping.
	WAITING_TASKS.lock().insert(tid, VecDeque::with_capacity(1));
	TASKS.lock().insert(
		tid,
		TaskHandle::new(
			tid,
			IDLE_PRIO,
			#[cfg(feature = "smp")]
			core_id,
		),
	);
	// Initialize a scheduler for this core.
	debug!(
		"Initializing scheduler for core {} with idle task {}",
		core_id, tid
	);
	let boxed_scheduler = Box::new(PerCoreScheduler {
		#[cfg(feature = "smp")]
		core_id,
		current_task: idle_task.clone(),
		idle_task: idle_task.clone(),
		#[cfg(target_arch = "x86_64")]
		fpu_owner: idle_task,
		ready_queue: PriorityTaskQueue::new(),
		finished_tasks: VecDeque::new(),
		blocked_tasks: BlockedTaskQueue::new(),
		#[cfg(feature = "tcp")]
		blocked_async_tasks: VecDeque::new(),
		#[cfg(feature = "smp")]
		input: InterruptTicketMutex::new(SchedulerInput::new()),
	});

	let scheduler = Box::into_raw(boxed_scheduler);
	set_core_scheduler(scheduler);
	#[cfg(feature = "smp")]
	{
		let scheduler = unsafe { scheduler.as_ref().unwrap() };
		SCHEDULER_INPUTS
			.lock()
			.insert(core_id.try_into().unwrap(), &scheduler.input);
	}
}

#[inline]
#[cfg(feature = "smp")]
fn get_scheduler_input(core_id: CoreId) -> &'static InterruptTicketMutex<SchedulerInput> {
	SCHEDULER_INPUTS.lock()[usize::try_from(core_id).unwrap()]
}

pub fn join(id: TaskId) -> Result<(), ()> {
	let core_scheduler = core_scheduler();

	debug!(
		"Task {} is waiting for task {}",
		core_scheduler.get_current_task_id(),
		id
	);

	{
		match WAITING_TASKS.lock().get_mut(&id) {
			Some(queue) => {
				queue.push_back(core_scheduler.get_current_task_handle());
				core_scheduler.block_current_task(None);
			}
			_ => {
				return Ok(());
			}
		}
	}

	// Switch to the next task.
	core_scheduler.reschedule();

	Ok(())
}

fn get_task_handle(id: TaskId) -> Option<TaskHandle> {
	TASKS.lock().get(&id).copied()
}
