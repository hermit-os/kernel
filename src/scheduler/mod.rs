// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//               2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::rc::Rc;
use core::cell::RefCell;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::arch;
use crate::arch::irq;
use crate::arch::mm::VirtAddr;
use crate::arch::percore::*;
use crate::arch::{switch_to_fpu_owner, switch_to_task};
use crate::collections::irqsave;
use crate::config::*;
use crate::scheduler::task::*;
use crate::synch::spinlock::*;

pub mod task;

static NO_TASKS: AtomicU32 = AtomicU32::new(0);
/// Map between Core ID and per-core scheduler
static mut SCHEDULERS: BTreeMap<CoreId, &PerCoreScheduler> = BTreeMap::new();
/// Map between Task ID and Task Control Block
static TASKS: SpinlockIrqSave<BTreeMap<TaskId, VecDeque<TaskHandle>>> =
	SpinlockIrqSave::new(BTreeMap::new());

/// Unique identifier for a core.
pub type CoreId = u32;

struct SchedulerInput {
	/// Queue of new tasks
	new_tasks: VecDeque<Rc<RefCell<Task>>>,
	/// Queue of task, which are wakeup by another core
	wakeup_tasks: VecDeque<TaskHandle>,
}

impl SchedulerInput {
	pub fn new() -> Self {
		Self {
			new_tasks: VecDeque::new(),
			wakeup_tasks: VecDeque::new(),
		}
	}
}
pub struct PerCoreScheduler {
	/// Core ID of this per-core scheduler
	core_id: CoreId,
	/// Task which is currently running
	current_task: Rc<RefCell<Task>>,
	/// Idle Task
	idle_task: Rc<RefCell<Task>>,
	/// Task that currently owns the FPU
	fpu_owner: Rc<RefCell<Task>>,
	/// Queue of tasks, which are ready
	ready_queue: PriorityTaskQueue,
	/// Queue of tasks, which are finished and can be released
	finished_tasks: VecDeque<Rc<RefCell<Task>>>,
	/// Queue of blocked tasks, sorted by wakeup time.
	blocked_tasks: BlockedTaskQueue,
	/// Queues to handle incoming requests from the other cores
	input: SpinlockIrqSave<SchedulerInput>,
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
		let task = Rc::new(RefCell::new(Task::new(
			tid,
			core_id,
			TaskStatus::TaskReady,
			prio,
			stack_size,
		)));
		task.borrow_mut().create_stack_frame(func, arg);

		// Add it to the task lists.
		let wakeup = {
			let mut input_locked = get_scheduler(core_id).input.lock();
			TASKS.lock().insert(tid, VecDeque::with_capacity(1));
			NO_TASKS.fetch_add(1, Ordering::SeqCst);

			if core_id != core_scheduler().core_id {
				input_locked.new_tasks.push_back(task);
				true
			} else {
				core_scheduler().ready_queue.push(task);
				false
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
				TaskStatus::TaskIdle,
				"Trying to terminate the idle task"
			);

			// Finish the task and reschedule.
			debug!(
				"Finishing task {} with exit code {}",
				current_task_borrowed.id, exit_code
			);
			current_task_borrowed.status = TaskStatus::TaskFinished;
			NO_TASKS.fetch_sub(1, Ordering::SeqCst);
		};

		irqsave(closure);

		self.scheduler();

		// we should never reach this point
		panic!("exit failed!")
	}

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
		let clone_task = Rc::new(RefCell::new(Task::clone(
			tid,
			core_id,
			&current_task_borrowed,
		)));
		clone_task.borrow_mut().create_stack_frame(func, arg);

		// Add it to the task lists.
		let wakeup = {
			let mut input_locked = get_scheduler(core_id).input.lock();
			TASKS.lock().insert(tid, VecDeque::with_capacity(1));
			NO_TASKS.fetch_add(1, Ordering::SeqCst);
			if core_id != core_scheduler().core_id {
				input_locked.new_tasks.push_back(clone_task);
				true
			} else {
				core_scheduler().ready_queue.push(clone_task);
				false
			}
		};

		debug!(
			"Creating task {} on core {} by cloning task {}",
			tid, core_id, current_task_borrowed.id
		);

		// Wake up the CPU
		if wakeup {
			arch::wakeup_core(core_id);
		}

		tid
	}

	pub fn clone(&self, func: extern "C" fn(usize), arg: usize) -> TaskId {
		irqsave(|| self.clone_impl(func, arg))
	}

	/// Returns `true` if a reschedule is required
	#[inline]
	pub fn is_scheduling(&self) -> bool {
		self.current_task.borrow().prio < self.ready_queue.get_highest_priority()
	}

	#[inline]
	pub fn handle_waiting_tasks(&mut self) {
		irqsave(|| self.blocked_tasks.handle_waiting_tasks());
	}

	pub fn custom_wakeup(&mut self, task: TaskHandle) {
		if task.get_core_id() == self.core_id {
			irqsave(|| self.blocked_tasks.custom_wakeup(task));
		} else {
			get_scheduler(task.get_core_id())
				.input
				.lock()
				.wakeup_tasks
				.push_back(task);
			// Wake up the CPU
			arch::wakeup_core(task.get_core_id());
		}
	}

	#[inline]
	pub fn block_current_task(&mut self, wakeup_time: Option<u64>) {
		irqsave(|| {
			self.blocked_tasks
				.add(self.current_task.clone(), wakeup_time)
		});
	}

	#[inline]
	pub fn get_current_task_handle(&self) -> TaskHandle {
		irqsave(|| {
			let current_task_borrowed = self.current_task.borrow();

			TaskHandle::new(
				current_task_borrowed.id,
				current_task_borrowed.prio,
				current_task_borrowed.core_id,
			)
		})
	}

	#[cfg(feature = "newlib")]
	#[inline]
	pub fn set_lwip_errno(&self, errno: i32) {
		irqsave(|| self.current_task.borrow_mut().lwip_errno = errno);
	}

	#[cfg(feature = "newlib")]
	#[inline]
	pub fn get_lwip_errno(&self) -> i32 {
		irqsave(|| self.current_task.borrow().lwip_errno)
	}

	#[inline]
	pub fn get_current_task_id(&self) -> TaskId {
		irqsave(|| self.current_task.borrow().id)
	}

	#[inline]
	pub fn get_current_task_prio(&self) -> Priority {
		irqsave(|| self.current_task.borrow().prio)
	}

	#[inline]
	pub fn get_current_task_wakeup_reason(&self) -> WakeupReason {
		irqsave(|| self.current_task.borrow_mut().last_wakeup_reason)
	}

	#[inline]
	pub fn set_current_task_wakeup_reason(&mut self, reason: WakeupReason) {
		irqsave(|| self.current_task.borrow_mut().last_wakeup_reason = reason);
	}

	#[inline]
	pub fn get_current_user_stack(&self) -> VirtAddr {
		self.current_task.borrow().user_stack_pointer
	}

	#[inline]
	pub fn set_current_user_stack(&mut self, addr: VirtAddr) {
		self.current_task.borrow_mut().user_stack_pointer = addr;
	}

	#[cfg(target_arch = "x86_64")]
	#[inline]
	pub fn get_current_kernel_stack(&self) -> VirtAddr {
		self.current_task.borrow().stacks.get_kernel_stack() + DEFAULT_STACK_SIZE - 0x10u64
	}

	#[cfg(target_arch = "x86_64")]
	pub fn set_current_kernel_stack(&self) {
		let current_task_borrowed = self.current_task.borrow();
		let tss = unsafe { &mut (*PERCORE.tss.get()) };

		tss.rsp[0] = (current_task_borrowed.stacks.get_kernel_stack()
			+ current_task_borrowed.stacks.get_kernel_stack_size()
			- 0x10u64)
			.as_u64();
		set_kernel_stack(tss.rsp[0]);
		tss.ist[0] = (current_task_borrowed.stacks.get_interupt_stack()
			+ current_task_borrowed.stacks.get_interupt_stack_size()
			- 0x10u64)
			.as_u64();
	}

	/// Save the FPU context for the current FPU owner and restore it for the current task,
	/// which wants to use the FPU now.
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
	/// Return true if a task is waked up
	fn cleanup_tasks(&mut self) -> bool {
		let mut result = false;

		// Pop the first finished task and remove it from the TASKS list, which implicitly deallocates all associated memory.
		while let Some(finished_task) = self.finished_tasks.pop_front() {
			let borrowed = finished_task.borrow();
			debug!("Cleaning up task {}", borrowed.id);

			// wakeup tasks, which are waiting for task with the identifier id
			match TASKS.lock().remove(&borrowed.id) {
				Some(mut queue) => {
					while let Some(task) = queue.pop_front() {
						result = true;
						self.custom_wakeup(task);
					}
				}
				None => {}
			}
		}

		result
	}

	pub fn check_input(&mut self) {
		let mut input_locked = self.input.lock();

		while let Some(task) = input_locked.wakeup_tasks.pop_front() {
			self.blocked_tasks.custom_wakeup(task);
		}

		while let Some(task) = input_locked.new_tasks.pop_front() {
			self.ready_queue.push(task.clone());
		}
	}

	/// Triggers the scheduler to reschedule the tasks.
	/// Interrupt flag will be cleared during the reschedule
	pub fn reschedule(&mut self) {
		irqsave(|| self.scheduler());
	}

	/// Only the idle task should call this function to
	/// reschedule the system. Set the idle task in halt
	/// state by leaving this function.
	pub fn reschedule_and_wait(&mut self) {
		irq::disable();
		self.scheduler();

		// do housekeeping
		let wakeup_tasks = self.cleanup_tasks();

		// Reenable interrupts and simultaneously set the CPU into the HALT state to only wake up at the next interrupt.
		// This atomic operation guarantees that we cannot miss a wakeup interrupt in between.
		if !wakeup_tasks {
			irq::enable_and_wait();
		} else {
			irq::enable();
		}
	}

	/// Triggers the scheduler to reschedule the tasks.
	/// Interrupt flag must be cleared before calling this function.
	pub fn scheduler(&mut self) {
		// Someone wants to give up the CPU
		// => we have time to cleanup the system
		let _ = self.cleanup_tasks();

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

		if status == TaskStatus::TaskRunning {
			// A task is currently running.
			// Check if a task with a equal or higher priority is available.
			if let Some(task) = self.ready_queue.pop_with_prio(prio) {
				new_task = Some(task);
			}
		} else {
			if status == TaskStatus::TaskFinished {
				// Mark the finished task as invalid and add it to the finished tasks for a later cleanup.
				self.current_task.borrow_mut().status = TaskStatus::TaskInvalid;
				self.finished_tasks.push_back(self.current_task.clone());
			}

			// No task is currently running.
			// Check if there is any available task and get the one with the highest priority.
			if let Some(task) = self.ready_queue.pop() {
				// This available task becomes the new task.
				debug!("Task is available.");
				new_task = Some(task);
			} else if status != TaskStatus::TaskIdle {
				// The Idle task becomes the new task.
				debug!("Only Idle Task is available.");
				new_task = Some(self.idle_task.clone());
			}
		}

		if let Some(task) = new_task {
			// There is a new task we want to switch to.

			// Handle the current task.
			if status == TaskStatus::TaskRunning {
				// Mark the running task as ready again and add it back to the queue.
				self.current_task.borrow_mut().status = TaskStatus::TaskReady;
				self.ready_queue.push(self.current_task.clone());
			}

			// Handle the new task and get information about it.
			let (new_id, new_stack_pointer, is_idle) = {
				let mut borrowed = task.borrow_mut();
				if borrowed.status != TaskStatus::TaskIdle {
					// Mark the new task as running.
					borrowed.status = TaskStatus::TaskRunning;
				}

				(
					borrowed.id,
					borrowed.last_stack_pointer,
					borrowed.status == TaskStatus::TaskIdle,
				)
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

				// Finally save our current context and restore the context of the new task.
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
		}
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
pub fn abort() {
	core_scheduler().exit(-1);
}

/// Add a per-core scheduler for the current core.
pub fn add_current_core() {
	// Create an idle task for this core.
	let core_id = core_id();
	let tid = get_tid();
	let idle_task = Rc::new(RefCell::new(Task::new_idle(tid, core_id)));

	// Add the ID -> Task mapping.
	TASKS.lock().insert(tid, VecDeque::with_capacity(1));
	// Initialize a scheduler for this core.
	debug!(
		"Initializing scheduler for core {} with idle task {}",
		core_id, tid
	);
	let boxed_scheduler = Box::new(PerCoreScheduler {
		core_id,
		current_task: idle_task.clone(),
		idle_task: idle_task.clone(),
		fpu_owner: idle_task,
		ready_queue: PriorityTaskQueue::new(),
		finished_tasks: VecDeque::new(),
		blocked_tasks: BlockedTaskQueue::new(),
		input: SpinlockIrqSave::new(SchedulerInput::new()),
	});

	let scheduler = Box::into_raw(boxed_scheduler);
	set_core_scheduler(scheduler);
	unsafe {
		SCHEDULERS.insert(core_id, &(*scheduler));
	}
}

#[inline]
fn get_scheduler(core_id: CoreId) -> &'static PerCoreScheduler {
	// Get the scheduler for the desired core.
	if let Some(result) = unsafe { SCHEDULERS.get(&core_id) } {
		result
	} else {
		panic!(
			"Trying to get the scheduler for core {}, but it isn't available",
			core_id
		);
	}
}

pub fn join(id: TaskId) -> Result<(), ()> {
	let core_scheduler = core_scheduler();

	debug!(
		"Task {} is waiting for task {}",
		core_scheduler.get_current_task_id(),
		id
	);

	{
		let mut guard = TASKS.lock();
		match guard.get_mut(&id) {
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
	core_scheduler.scheduler();

	Ok(())
}
