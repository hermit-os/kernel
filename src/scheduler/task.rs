// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//               2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use alloc::rc::Rc;
use arch;
use arch::mm::paging::{BasePageSize, PageSize};
use arch::processor::msb;
use arch::scheduler::TaskStacks;
use collections::{DoublyLinkedList, Node};
use core::cell::RefCell;
use core::fmt;
use mm;
use scheduler;
use synch::spinlock::SpinlockIrqSave;

/// The status of the task - used for scheduling
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TaskStatus {
	TaskInvalid,
	TaskReady,
	TaskRunning,
	TaskBlocked,
	TaskFinished,
	TaskIdle,
}

/// Reason why wakeup() has been called on a task.
#[derive(Clone, Copy, PartialEq)]
pub enum WakeupReason {
	Custom,
	Timer,
	All,
}

/// Unique identifier for a task (i.e. `pid`).
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Copy)]
pub struct TaskId(u32);

impl TaskId {
	pub const fn into(self) -> u32 {
		self.0
	}

	pub const fn from(x: u32) -> Self {
		TaskId(x)
	}
}

impl fmt::Display for TaskId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}", self.0)
	}
}

/// Priority of a task
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Copy)]
pub struct Priority(u8);

impl Priority {
	pub const fn into(self) -> u8 {
		self.0
	}

	pub const fn from(x: u8) -> Self {
		Priority(x)
	}
}

impl fmt::Display for Priority {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}", self.0)
	}
}

#[allow(dead_code)]
pub const HIGH_PRIO: Priority = Priority::from(3);
#[allow(dead_code)]
pub const NORMAL_PRIO: Priority = Priority::from(2);
#[allow(dead_code)]
pub const LOW_PRIO: Priority = Priority::from(1);
#[allow(dead_code)]
pub const IDLE_PRIO: Priority = Priority::from(0);

/// Maximum number of priorities
pub const NO_PRIORITIES: usize = 31;

struct QueueHead {
	head: Option<Rc<RefCell<Task>>>,
	tail: Option<Rc<RefCell<Task>>>,
}

impl QueueHead {
	pub const fn new() -> Self {
		QueueHead {
			head: None,
			tail: None,
		}
	}
}

impl Default for QueueHead {
	fn default() -> Self {
		Self {
			head: None,
			tail: None,
		}
	}
}

/// Realize a priority queue for tasks
pub struct PriorityTaskQueue {
	queues: [QueueHead; NO_PRIORITIES],
	prio_bitmap: u64,
}

impl PriorityTaskQueue {
	/// Creates an empty priority queue for tasks
	pub const fn new() -> PriorityTaskQueue {
		PriorityTaskQueue {
			queues: [
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
				QueueHead::new(),
			],
			prio_bitmap: 0,
		}
	}

	/// Add a task by its priority to the queue
	pub fn push(&mut self, task: Rc<RefCell<Task>>) {
		let i = task.borrow().prio.into() as usize;
		//assert!(i < NO_PRIORITIES, "Priority {} is too high", i);

		self.prio_bitmap |= 1 << i;
		match self.queues[i].tail {
			None => {
				// first element in the queue
				self.queues[i].head = Some(task.clone());

				let mut borrow = task.borrow_mut();
				borrow.next = None;
				borrow.prev = None;
			}
			Some(ref mut tail) => {
				// add task at the end of the node
				tail.borrow_mut().next = Some(task.clone());

				let mut borrow = task.borrow_mut();
				borrow.next = None;
				borrow.prev = Some(tail.clone());
			}
		}

		self.queues[i].tail = Some(task.clone());
	}

	fn pop_from_queue(&mut self, queue_index: usize) -> Option<Rc<RefCell<Task>>> {
		let new_head;
		let task;

		match self.queues[queue_index].head {
			None => {
				return None;
			}
			Some(ref mut head) => {
				let mut borrow = head.borrow_mut();

				match borrow.next {
					Some(ref mut nhead) => {
						nhead.borrow_mut().prev = None;
					}
					None => {}
				}

				new_head = borrow.next.clone();
				borrow.next = None;
				borrow.prev = None;

				task = head.clone();
			}
		}

		self.queues[queue_index].head = new_head;
		if self.queues[queue_index].head.is_none() {
			self.queues[queue_index].tail = None;
			self.prio_bitmap &= !(1 << queue_index as u64);
		}

		Some(task)
	}

	/// Pop the task with the highest priority from the queue
	pub fn pop(&mut self) -> Option<Rc<RefCell<Task>>> {
		if let Some(i) = msb(self.prio_bitmap) {
			return self.pop_from_queue(i as usize);
		}

		None
	}

	/// Pop the next task, which has a higher or the same priority as `prio`
	pub fn pop_with_prio(&mut self, prio: Priority) -> Option<Rc<RefCell<Task>>> {
		if let Some(i) = msb(self.prio_bitmap) {
			if i >= u64::from(prio.into()) {
				return self.pop_from_queue(i as usize);
			}
		}

		None
	}

	/// Remove a specific task from the priority queue.
	pub fn remove(&mut self, task: Rc<RefCell<Task>>) {
		let i = task.borrow().prio.into() as usize;
		//assert!(i < NO_PRIORITIES, "Priority {} is too high", i);

		let mut curr = self.queues[i].head.clone();
		let mut next_curr;

		loop {
			match curr {
				None => {
					break;
				}
				Some(ref curr_task) => {
					if Rc::ptr_eq(&curr_task, &task) {
						let (mut prev, mut next) = {
							let borrowed = curr_task.borrow_mut();
							(borrowed.prev.clone(), borrowed.next.clone())
						};

						match prev {
							Some(ref mut t) => {
								t.borrow_mut().next = next.clone();
							}
							None => {}
						};

						match next {
							Some(ref mut t) => {
								t.borrow_mut().prev = prev.clone();
							}
							None => {}
						};

						break;
					}

					next_curr = curr_task.borrow().next.clone();
				}
			}

			curr = next_curr.clone();
		}

		let new_head = match self.queues[i].head {
			Some(ref curr_task) => Rc::ptr_eq(&curr_task, &task),
			None => false,
		};

		if new_head {
			self.queues[i].head = task.borrow().next.clone();

			if self.queues[i].head.is_none() {
				self.prio_bitmap &= !(1 << i as u64);
			}
		}
	}
}

pub struct TaskTLS {
	address: usize,
	size: usize,
}

impl TaskTLS {
	pub fn new(size: usize) -> Self {
		// We allocate in BasePageSize granularity, so we don't have to manually impose an
		// additional alignment for TLS variables.
		let memory_size = align_up!(size, BasePageSize::SIZE);
		Self {
			address: mm::allocate(memory_size, true),
			size: memory_size,
		}
	}

	pub fn address(&self) -> usize {
		self.address
	}
}

impl Drop for TaskTLS {
	fn drop(&mut self) {
		debug!(
			"Deallocate TLS at 0x{:x} (size 0x{:x})",
			self.address, self.size
		);
		mm::deallocate(self.address, self.size);
	}
}

/// A task control block, which identifies either a process or a thread
#[repr(align(64))]
pub struct Task {
	/// The ID of this context
	pub id: TaskId,
	/// Status of a task, e.g. if the task is ready or blocked
	pub status: TaskStatus,
	/// Task priority,
	pub prio: Priority,
	/// Last stack pointer before a context switch to another task
	pub last_stack_pointer: usize,
	/// Last FPU state before a context switch to another task using the FPU
	pub last_fpu_state: arch::processor::FPUState,
	/// ID of the core this task is running on
	pub core_id: usize,
	/// Stack of the task
	pub stacks: TaskStacks,
	/// next task in queue
	pub next: Option<Rc<RefCell<Task>>>,
	/// previous task in queue
	pub prev: Option<Rc<RefCell<Task>>>,
	/// list of waiting tasks
	pub wakeup: SpinlockIrqSave<BlockedTaskQueue>,
	/// Task Thread-Local-Storage (TLS)
	pub tls: Option<Rc<RefCell<TaskTLS>>>,
	/// Reason why wakeup() has been called the last time
	pub last_wakeup_reason: WakeupReason,
	/// lwIP error code for this task
	#[cfg(feature = "newlib")]
	pub lwip_errno: i32,
}

pub trait TaskFrame {
	/// Create the initial stack frame for a new task
	fn create_stack_frame(&mut self, func: extern "C" fn(usize), arg: usize);
}

impl Task {
	pub fn new(tid: TaskId, core_id: usize, task_status: TaskStatus, task_prio: Priority) -> Task {
		debug!("Creating new task {}", tid);

		Task {
			id: tid,
			status: task_status,
			prio: task_prio,
			last_stack_pointer: 0,
			last_fpu_state: arch::processor::FPUState::new(),
			core_id: core_id,
			stacks: TaskStacks::new(),
			next: None,
			prev: None,
			wakeup: SpinlockIrqSave::new(BlockedTaskQueue::new()),
			tls: None,
			last_wakeup_reason: WakeupReason::Custom,
			#[cfg(feature = "newlib")]
			lwip_errno: 0,
		}
	}

	pub fn new_idle(tid: TaskId, core_id: usize) -> Task {
		debug!("Creating idle task {}", tid);

		Task {
			id: tid,
			status: TaskStatus::TaskIdle,
			prio: IDLE_PRIO,
			last_stack_pointer: 0,
			last_fpu_state: arch::processor::FPUState::new(),
			core_id: core_id,
			stacks: TaskStacks::from_boot_stacks(),
			next: None,
			prev: None,
			wakeup: SpinlockIrqSave::new(BlockedTaskQueue::new()),
			tls: None,
			last_wakeup_reason: WakeupReason::Custom,
			#[cfg(feature = "newlib")]
			lwip_errno: 0,
		}
	}

	pub fn clone(tid: TaskId, core_id: usize, task: &Task) -> Task {
		debug!("Cloning task {} from task {}", tid, task.id);

		Task {
			id: tid,
			status: TaskStatus::TaskReady,
			prio: task.prio,
			last_stack_pointer: 0,
			last_fpu_state: arch::processor::FPUState::new(),
			core_id: core_id,
			stacks: TaskStacks::new(),
			next: None,
			prev: None,
			wakeup: SpinlockIrqSave::new(BlockedTaskQueue::new()),
			tls: task.tls.clone(),
			last_wakeup_reason: task.last_wakeup_reason,
			#[cfg(feature = "newlib")]
			lwip_errno: 0,
		}
	}
}

struct BlockedTask {
	task: Rc<RefCell<Task>>,
	wakeup_time: Option<u64>,
}

pub struct BlockedTaskQueue {
	list: DoublyLinkedList<BlockedTask>,
}

impl BlockedTaskQueue {
	pub const fn new() -> Self {
		Self {
			list: DoublyLinkedList::new(),
		}
	}

	fn wakeup_task(task: Rc<RefCell<Task>>, reason: WakeupReason) {
		// Get the Core ID of the task to wake up.
		let core_id = {
			let mut borrowed = task.borrow_mut();
			debug!(
				"Waking up task {} on core {}",
				borrowed.id, borrowed.core_id
			);

			assert!(
				borrowed.status == TaskStatus::TaskBlocked,
				"Trying to wake up task {} which is not blocked",
				borrowed.id
			);
			borrowed.status = TaskStatus::TaskReady;
			borrowed.last_wakeup_reason = reason;

			borrowed.core_id
		};

		// Get the scheduler of that core.
		let core_scheduler = scheduler::get_scheduler(core_id);

		// Add the task to the ready queue.
		let is_halted = {
			let mut state_locked = core_scheduler.state.lock();
			state_locked.ready_queue.push(task);
			state_locked.is_halted
		};

		// Wake up the CPU if needed.
		if is_halted {
			arch::wakeup_core(core_id);
		}
	}

	/// Blocks the given task for `wakeup_time` ticks, or indefinitely if None is given.
	pub fn add(&mut self, task: Rc<RefCell<Task>>, wakeup_time: Option<u64>) {
		{
			// Set the task status to Blocked.
			let mut borrowed = task.borrow_mut();
			debug!("Blocking task {}", borrowed.id);

			assert!(
				borrowed.status == TaskStatus::TaskRunning,
				"Trying to block task {} which is not running",
				borrowed.id
			);
			borrowed.status = TaskStatus::TaskBlocked;
		}

		let new_node = Node::new(BlockedTask {
			task: task,
			wakeup_time: wakeup_time,
		});

		// Shall the task automatically be woken up after a certain time?
		if let Some(wt) = wakeup_time {
			let mut first_task = true;

			// Yes, then insert it at the right position into the list sorted by wakeup time.
			for node in self.list.iter() {
				let node_wakeup_time = node.borrow().value.wakeup_time;
				if node_wakeup_time.is_none() || wt < node_wakeup_time.unwrap() {
					self.list.insert_before(new_node, node);

					// If this is the new first task in the list, update the One-Shot Timer
					// to fire when this task shall be woken up.
					if first_task {
						arch::set_oneshot_timer(wakeup_time);
					}

					return;
				}

				first_task = false;
			}

			// The right position is at the end of the list or the list is empty.
			self.list.push(new_node);
			if first_task {
				arch::set_oneshot_timer(wakeup_time);
			}
		} else {
			// No, then just insert it at the end of the list.
			self.list.push(new_node);
		}
	}

	/// Wakeup all blocked tasks
	pub fn wakeup_all(&mut self) {
		// Loop through all blocked tasks to find it.
		for node in self.list.iter() {
			// Remove it from the list of blocked tasks and wake it up.
			self.list.remove(node.clone());
			Self::wakeup_task(node.borrow().value.task.clone(), WakeupReason::All);
		}
	}

	/// Manually wake up a blocked task.
	pub fn custom_wakeup(&mut self, task: Rc<RefCell<Task>>) {
		let mut first_task = true;
		let mut iter = self.list.iter();

		// Loop through all blocked tasks to find it.
		while let Some(node) = iter.next() {
			if Rc::ptr_eq(&node.borrow().value.task, &task) {
				// Remove it from the list of blocked tasks and wake it up.
				self.list.remove(node.clone());
				Self::wakeup_task(task, WakeupReason::Custom);

				// If this is the first task, adjust the One-Shot Timer to fire at the
				// next task's wakeup time (if any).
				if first_task {
					if let Some(next_node) = iter.next() {
						arch::set_oneshot_timer(next_node.borrow().value.wakeup_time);
					}
				}

				break;
			}

			first_task = false;
		}
	}

	/// Wakes up all tasks whose wakeup time has elapsed.
	///
	/// Should be called by the One-Shot Timer interrupt handler when the wakeup time for
	/// at least one task has elapsed.
	pub fn handle_waiting_tasks(&mut self) {
		// Get the current time.
		let time = arch::processor::get_timer_ticks();

		// Loop through all blocked tasks.
		for node in self.list.iter() {
			// Get the wakeup time of this task and check if we have reached the first task
			// that hasn't elapsed yet or waits indefinitely.
			let node_wakeup_time = node.borrow().value.wakeup_time;
			if node_wakeup_time.is_none() || time < node_wakeup_time.unwrap() {
				// Adjust the One-Shot Timer to fire at this task's wakeup time (if any)
				// and exit the loop.
				arch::set_oneshot_timer(node_wakeup_time);
				break;
			}

			// Otherwise, this task has elapsed, so remove it from the list and wake it up.
			self.list.remove(node.clone());
			Self::wakeup_task(node.borrow().value.task.clone(), WakeupReason::Timer);
		}
	}
}
