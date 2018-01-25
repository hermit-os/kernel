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
use arch::irq;
use arch::percore::*;
use core::cell::RefCell;
use core::sync::atomic::{AtomicUsize, Ordering};
use scheduler::task::*;
use synch::spinlock::*;

extern "C" {
	fn switch(old_stack: *const usize, new_stack: usize);
}


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
}

impl PerCoreScheduler {
	pub fn get_current_task(&self) -> Rc<RefCell<Task>> {
		self.current_task.clone()
	}

	/// Spawn a new task.
	pub fn spawn(&self, func: extern "C" fn(usize), arg: usize, prio: Priority, heap_start: Option<usize>) -> TaskId {
		// Create the new task.
		let tid = get_tid();
		let mut task = Rc::new(RefCell::new(Task::new(tid, self.core_id, TaskStatus::TaskReady, prio, heap_start)));
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

		self.reschedule();

		// we should never reach this point
		panic!("exit failed!")
	}

	/// Block the current task.
	pub fn block(&mut self) -> Rc<RefCell<Task>> {
		// Get the current task.
		let mut task_borrowed = self.current_task.borrow_mut();
		assert!(task_borrowed.status == TaskStatus::TaskRunning, "Trying to block a task which is not running");

		// Block the task.
		info!("Blocking task {}", task_borrowed.id);
		task_borrowed.status = TaskStatus::TaskBlocked;
		self.current_task.clone()
	}

	/// Wake up a specified blocked task.
	pub fn wakeup(&mut self, task: Rc<RefCell<Task>>) {
		let prio = {
			let mut task_borrowed = task.borrow_mut();
			assert!(task_borrowed.core_id == self.core_id, "Trying to wake up task {} which isn't scheduled on this core", task_borrowed.id);
			assert!(task_borrowed.status == TaskStatus::TaskBlocked, "Trying to wake up task {} which is not blocked", task_borrowed.id);

			info!("Waking up task {}", task_borrowed.id);
			task_borrowed.status = TaskStatus::TaskReady;
			task_borrowed.prio
		};

		self.ready_queue.lock().push(prio, task);
	}

	pub fn clone(&self, func: extern "C" fn(usize), arg: usize) -> TaskId {
		// Get the scheduler for the next available core.
		let schedulers = unsafe { SCHEDULERS.as_mut().unwrap() };
		let mut iter = schedulers.iter_mut();
		let mut next_scheduler = iter.next().unwrap().1;
		for (id, scheduler) in iter {
			if *id > self.core_id {
				next_scheduler = scheduler;
				break;
			}
		}

		// Get the current task.
		let task_borrowed = self.current_task.borrow();

		// Clone the current task.
		let tid = get_tid();
		let mut task = Rc::new(RefCell::new(Task::clone(tid, next_scheduler.core_id, &task_borrowed)));
		task.borrow_mut().create_stack_frame(func, arg);

		// Add it to the task lists.
		next_scheduler.ready_queue.lock().push(task_borrowed.prio, task.clone());
		unsafe { TASKS.as_ref().unwrap().lock().insert(tid, task); }
		NO_TASKS.fetch_add(1, Ordering::SeqCst);

		info!("Creating task {} by cloning task {}", tid, task_borrowed.id);
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

	fn get_next_task(&mut self) -> Option<Rc<RefCell<Task>>> {
		let (prio, status) = {
			let borrowed = self.current_task.borrow();

			// If the current task is runnable, we look for a task with this or a higher priority.
			let mut prio = LOW_PRIO;
			if borrowed.status == TaskStatus::TaskRunning {
				prio = borrowed.prio;
			}

			(prio, borrowed.status)
		};

		if let Some(task) = self.ready_queue.lock().pop_with_prio(prio) {
			// Return the task with this or a higher priority.
			task.borrow_mut().status = TaskStatus::TaskRunning;
			Some(task)
		} else if status != TaskStatus::TaskRunning && status != TaskStatus::TaskIdle {
			// Current task isn't able to run and no other task available.
			// => switch to the idle task
			Some(self.idle_task.clone())
		} else {
			// No task is available at all.
			None
		}
	}

	fn schedule(&mut self) {
		// Do we have a task, which is ready?
		if let Some(task) = self.get_next_task() {
			let (old_id, old_stack_pointer) = {
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

			let (new_id, new_stack_pointer) = {
				let borrowed = task.borrow();
				(borrowed.id, borrowed.last_stack_pointer)
			};

			debug!("switch task from {} to {}", old_id, new_id);
			self.current_task = task;
			unsafe { switch(old_stack_pointer, new_stack_pointer); }
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

	/// Triggers the scheduler to reschedule the tasks
	#[inline(always)]
	pub fn reschedule(&mut self) {
		// Someone wants to give up the CPU
		// => we have time to cleanup the system
		self.cleanup_tasks();

		let flags = irq::nested_disable();
		self.schedule();
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
	let per_core_scheduler = PerCoreScheduler {
		core_id: core_id,
		current_task: idle_task.clone(),
		idle_task: idle_task.clone(),
		fpu_owner: idle_task,
		ready_queue: SpinlockIrqSave::new(PriorityTaskQueue::new()),
		finished_tasks: SpinlockIrqSave::new(VecDeque::new()),
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
