// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//               2018 Colin Finck, RWTH Aachen University
//
// MIT License
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
// LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
// WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

pub mod task;

use alloc::btree_map::*;
use alloc::rc::Rc;
use alloc::VecDeque;
use arch;
use arch::irq;
use arch::percore::*;
use core::cell::RefCell;
use core::sync::atomic::{AtomicUsize, Ordering};
use scheduler::task::*;
use synch::spinlock::*;

extern "C" {
	fn switch(old_stack: *const usize, new_stack: usize);
}


static NEXT_CPU_NUMBER: AtomicUsize = AtomicUsize::new(1);
static NO_TASKS: AtomicUsize = AtomicUsize::new(0);
/// Map between Core ID and per-core scheduler
static mut SCHEDULERS: Option<BTreeMap<u32, PerCoreScheduler>> = None;
/// Map between Task ID and Task Control Block
static mut TASKS: Option<SpinlockIrqSave<BTreeMap<TaskId, Rc<RefCell<Task>>>>> = None;
static TID_COUNTER: AtomicUsize = AtomicUsize::new(0);


pub struct PerCoreScheduler {
	/// Core ID of this per-core scheduler
	core_id: u32,
	/// Task which is currently running
	current_task: Rc<RefCell<Task>>,
	/// Idle Task
	idle_task: Rc<RefCell<Task>>,
	/// Task that currently owns the FPU
	fpu_owner: Rc<RefCell<Task>>,
	/// Queue of tasks, which are ready
	ready_queue: SpinlockIrqSave<PriorityTaskQueue>,
	/// Queue of tasks, which are finished and can be released
	finished_tasks: SpinlockIrqSave<VecDeque<TaskId>>,
	/// Queue of blocked tasks, sorted by wakeup time.
	pub blocked_tasks: SpinlockIrqSave<BlockedTaskQueue>,
	/// Processor Timer Tick when we last switched the current task.
	last_task_switch_tick: usize,
}

impl PerCoreScheduler {
	pub fn get_current_task(&self) -> Rc<RefCell<Task>> {
		self.current_task.clone()
	}

	/// Spawn a new task.
	pub fn spawn(&self, func: extern "C" fn(usize), arg: usize, prio: Priority, heap_start: Option<usize>) -> TaskId {
		// Create the new task.
		let tid = get_tid();
		let task = Rc::new(RefCell::new(Task::new(tid, self.core_id, TaskStatus::TaskReady, prio, heap_start)));
		task.borrow_mut().create_stack_frame(func, arg);

		// Add it to the task lists.
		self.ready_queue.lock().push(prio, task.clone());
		unsafe { TASKS.as_ref().unwrap().lock().insert(tid, task); }
		NO_TASKS.fetch_add(1, Ordering::SeqCst);

		info!("Creating task {}", tid);
		tid
	}

	/// Terminate the current task on the current core.
	pub fn exit(&mut self, exit_code: i32) -> ! {
		{
			// Get the current task.
			let mut task_borrowed = self.current_task.borrow_mut();
			assert!(task_borrowed.status != TaskStatus::TaskIdle, "Trying to terminate the idle task");

			// Finish the task and reschedule.
			info!("Finishing task {} with exit code {}", task_borrowed.id, exit_code);
			task_borrowed.status = TaskStatus::TaskFinished;
			NO_TASKS.fetch_sub(1, Ordering::SeqCst);
		}

		self.scheduler();

		// we should never reach this point
		panic!("exit failed!")
	}

	pub fn clone(&self, func: extern "C" fn(usize), arg: usize) -> TaskId {
		// Get the Core ID of the next CPU.
		let core_id = {
			// Increase the CPU number by 1.
			let cpu_number = NEXT_CPU_NUMBER.fetch_add(1, Ordering::SeqCst);

			// Translate this CPU number to a Core ID.
			// Both numbers often match, but don't need to (e.g. when a Core has been disabled).
			match arch::get_core_id_for_cpu_number(cpu_number) {
				Some(core_id) => {
					core_id
				},
				None => {
					// This CPU number does not exist, so start over again with CPU number 0 = Core ID 0.
					NEXT_CPU_NUMBER.store(0, Ordering::SeqCst);
					0
				}
			}
		};

		// Get the scheduler of that core.
		let next_scheduler = get_scheduler(core_id);

		// Get the current task.
		let task_borrowed = self.current_task.borrow();

		// Clone the current task.
		let tid = get_tid();
		let task = Rc::new(RefCell::new(Task::clone(tid, core_id, &task_borrowed)));
		task.borrow_mut().create_stack_frame(func, arg);

		// Add it to the task lists.
		next_scheduler.ready_queue.lock().push(task_borrowed.prio, task.clone());
		unsafe { TASKS.as_ref().unwrap().lock().insert(tid, task); }
		NO_TASKS.fetch_add(1, Ordering::SeqCst);

		info!("Creating task {} on core {} by cloning task {}", tid, core_id, task_borrowed.id);

		// If that CPU has been running the Idle task, it may be in a HALT state and needs to be woken up.
		if next_scheduler.current_task.borrow().status == TaskStatus::TaskIdle {
			arch::wakeup_core(core_id);
		}

		tid
	}

	/// Save the FPU context for the current FPU owner and restore it for the current task,
	/// which wants to use the FPU now.
	pub fn fpu_switch(&mut self) {
		if !Rc::ptr_eq(&self.current_task, &self.fpu_owner) {
			debug!("Switching FPU owner from task {} to {}", self.fpu_owner.borrow().id, self.current_task.borrow().id);

			self.fpu_owner.borrow_mut().last_fpu_state.save();
			self.current_task.borrow().last_fpu_state.restore();
			self.fpu_owner = self.current_task.clone();
		}
	}

	/// Check if a finished task could be deleted.
	fn cleanup_tasks(&mut self) {
		// Pop the first finished task and remove it from the TASKS list, which implicitly deallocates all associated memory.
		if let Some(id) = self.finished_tasks.lock().pop_front() {
			info!("Cleaning up task {}", id);
			unsafe { TASKS.as_ref().unwrap().lock().remove(&id); }
		}
	}

	fn switch_to_task(&mut self, new_task: Rc<RefCell<Task>>) {
		// Handle the current task and get information about it.
		let (id, last_stack_pointer) = {
			let mut borrowed = self.current_task.borrow_mut();

			if borrowed.status == TaskStatus::TaskRunning {
				// Mark the running task as ready again and add it back to the queue.
				borrowed.status = TaskStatus::TaskReady;
				self.ready_queue.lock().push(borrowed.prio, self.current_task.clone());
			} else if borrowed.status == TaskStatus::TaskFinished {
				// Mark the finished task as invalid and add it to the finished tasks for a later cleanup.
				borrowed.status = TaskStatus::TaskInvalid;
				self.finished_tasks.lock().push_back(borrowed.id);
			}

			(borrowed.id, &borrowed.last_stack_pointer as *const usize)
		};

		// Handle the new task and get information about it.
		let (new_id, new_stack_pointer) = {
			let mut borrowed = new_task.borrow_mut();

			if borrowed.status != TaskStatus::TaskIdle {
				// Mark the new task as running.
				borrowed.status = TaskStatus::TaskRunning;
			}

			(borrowed.id, borrowed.last_stack_pointer)
		};

		// Finally do the switch.
		debug!("Switching task from {} to {} ({:#X}, *{:#X} => {:#X})", id, new_id,
			last_stack_pointer as usize, unsafe { *last_stack_pointer }, new_stack_pointer);
		self.current_task = new_task;
		self.last_task_switch_tick = arch::processor::update_timer_ticks();
		unsafe { switch(last_stack_pointer, new_stack_pointer); }
	}

	/// Triggers the scheduler to reschedule the tasks
	pub fn scheduler(&mut self) {
		// Someone wants to give up the CPU
		// => we have time to cleanup the system
		self.cleanup_tasks();

		let flags = irq::nested_disable();
		let mut new_task = None;

		// Get information about the current task.
		let (prio, status) = {
			let borrowed = self.current_task.borrow();
			(borrowed.prio, borrowed.status)
		};

		if status == TaskStatus::TaskRunning {
			// A task is currently running.
			// Check if a task with a higher priority is available.
			let higher_prio = Priority::from(prio.into() + 1);
			if let Some(task) = self.ready_queue.lock().pop_with_prio(higher_prio) {
				// This higher priority task becomes the new task.
				debug!("Task with a higher priority is available.");
				new_task = Some(task);
			} else {
				// No task with a higher priority is available, but a task with the same priority as ours may be available.
				// We implement Round-Robin Scheduling for this case.
				// Check if our current task has been running for at least a single timer tick.
				if arch::processor::update_timer_ticks() > self.last_task_switch_tick {
					// Check if a task with our own priority is available.
					if let Some(task) = self.ready_queue.lock().pop_with_prio(prio) {
						// This task becomes the new task.
						debug!("Time slice expired for current task.");
						new_task = Some(task);
					}
				}
			}
		} else {
			// No task is currently running.
			// Check if there is any available task and get the one with the highest priority.
			if let Some(task) = self.ready_queue.lock().pop() {
				// This available task becomes the new task.
				debug!("Task is available.");
				new_task = Some(task);
			} else if status != TaskStatus::TaskIdle {
				// The Idle task becomes the new task.
				debug!("Only Idle Task is available.");
				new_task = Some(self.idle_task.clone());
			}
		}

		// If we set a new task, switch to this task.
		if let Some(task) = new_task {
			self.switch_to_task(task);
		}

		irq::nested_enable(flags);
	}
}

fn get_tid() -> TaskId {
	loop {
		let id = TaskId::from(TID_COUNTER.fetch_add(1, Ordering::SeqCst));
		if unsafe { !TASKS.as_ref().unwrap().lock().contains_key(&id) } {
			return id;
		}
	}
}

pub fn init() {
	unsafe {
		SCHEDULERS = Some(BTreeMap::new());
		TASKS = Some(SpinlockIrqSave::new(BTreeMap::new()));
	}
}

#[inline]
pub fn abort() {
	get_scheduler(core_id()).exit(-1);
}

/// Add a per-core scheduler for the current core.
pub fn add_current_core() {
	// Create an idle task for this core.
	let core_id = core_id();
	let tid = get_tid();
	let idle_task = Rc::new(RefCell::new(Task::new_idle(tid, core_id)));

	// Add the ID -> Task mapping.
	unsafe { TASKS.as_ref().unwrap().lock().insert(tid, idle_task.clone()); }

	// Initialize a scheduler for this core.
	debug!("Initializing scheduler for this core with idle task {}", tid);
	let per_core_scheduler = PerCoreScheduler {
		core_id: core_id,
		current_task: idle_task.clone(),
		idle_task: idle_task.clone(),
		fpu_owner: idle_task,
		ready_queue: SpinlockIrqSave::new(PriorityTaskQueue::new()),
		finished_tasks: SpinlockIrqSave::new(VecDeque::new()),
		blocked_tasks: SpinlockIrqSave::new(BlockedTaskQueue::new()),
		last_task_switch_tick: 0,
	};
	unsafe { SCHEDULERS.as_mut().unwrap().insert(core_id, per_core_scheduler); }
}

pub fn get_scheduler(core_id: u32) -> &'static mut PerCoreScheduler {
	// Get the scheduler for the desired core.
	let result = unsafe { SCHEDULERS.as_mut().unwrap().get_mut(&core_id) };
	assert!(result.is_some(), "Trying to get the scheduler for core {}, but it isn't available", core_id);
	result.unwrap()
}

/// Return the current number of tasks.
pub fn number_of_tasks() -> usize {
	NO_TASKS.load(Ordering::SeqCst)
}
