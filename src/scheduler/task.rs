// Copyright (c) 2017 Stefan Lankes, RWTH Aachen University
//               2018 Colin Finck, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::arch;
use crate::arch::mm::VirtAddr;
use crate::arch::percore::*;
use crate::arch::scheduler::{TaskStacks, TaskTLS};
use crate::scheduler::CoreId;
use alloc::collections::{LinkedList, VecDeque};
use alloc::rc::Rc;
use core::cell::RefCell;
use core::cmp::Ordering;
use core::convert::TryInto;
use core::fmt;
use core::num::NonZeroU64;

/// Returns the most significant bit.
///
/// # Examples
///
/// ```
/// assert_eq!(msb(0), None);
/// assert_eq!(msb(1), 0);
/// assert_eq!(msb(u64::MAX), 63);
/// ```
#[inline]
fn msb(n: u64) -> Option<u32> {
	NonZeroU64::new(n).map(|n| u64::BITS - 1 - n.leading_zeros())
}

/// The status of the task - used for scheduling
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TaskStatus {
	Invalid,
	Ready,
	Running,
	Blocked,
	Finished,
	Idle,
}

/// Reason why wakeup() has been called on a task.
#[derive(Clone, Copy, PartialEq)]
pub enum WakeupReason {
	Custom,
	Timer,
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
pub const NORMAL_PRIO: Priority = Priority::from(2);
#[allow(dead_code)]
pub const LOW_PRIO: Priority = Priority::from(1);
pub const IDLE_PRIO: Priority = Priority::from(0);

/// Maximum number of priorities
pub const NO_PRIORITIES: usize = 31;

#[derive(Copy, Clone, Debug)]
pub struct TaskHandle {
	id: TaskId,
	priority: Priority,
	core_id: CoreId,
}

impl TaskHandle {
	pub fn new(id: TaskId, priority: Priority, core_id: CoreId) -> Self {
		Self {
			id,
			priority,
			core_id,
		}
	}

	pub fn get_core_id(&self) -> CoreId {
		self.core_id
	}

	pub fn get_id(&self) -> TaskId {
		self.id
	}

	pub fn get_priority(&self) -> Priority {
		self.priority
	}
}

impl Ord for TaskHandle {
	fn cmp(&self, other: &Self) -> Ordering {
		self.id.cmp(&other.id)
	}
}

impl PartialOrd for TaskHandle {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}

impl PartialEq for TaskHandle {
	fn eq(&self, other: &Self) -> bool {
		self.id == other.id
	}
}

impl Eq for TaskHandle {}

/// Realize a priority queue for task handles
pub struct TaskHandlePriorityQueue {
	queues: [Option<VecDeque<TaskHandle>>; NO_PRIORITIES],
	prio_bitmap: u64,
}

impl TaskHandlePriorityQueue {
	/// Creates an empty priority queue for tasks
	pub const fn new() -> Self {
		Self {
			queues: [
				None, None, None, None, None, None, None, None, None, None, None, None, None, None,
				None, None, None, None, None, None, None, None, None, None, None, None, None, None,
				None, None, None,
			],
			prio_bitmap: 0,
		}
	}

	/// Add a task handle by its priority to the queue
	pub fn push(&mut self, task: TaskHandle) {
		let i = task.priority.into() as usize;
		//assert!(i < NO_PRIORITIES, "Priority {} is too high", i);

		self.prio_bitmap |= (1 << i) as u64;
		if let Some(queue) = &mut self.queues[i] {
			queue.push_back(task);
		} else {
			let mut queue = VecDeque::new();
			queue.push_back(task);
			self.queues[i] = Some(queue);
		}
	}

	fn pop_from_queue(&mut self, queue_index: usize) -> Option<TaskHandle> {
		if let Some(queue) = &mut self.queues[queue_index] {
			let task = queue.pop_front();

			if queue.is_empty() {
				self.prio_bitmap &= !(1 << queue_index as u64);
			}

			task
		} else {
			None
		}
	}

	/// Pop the task handle with the highest priority from the queue
	pub fn pop(&mut self) -> Option<TaskHandle> {
		if let Some(i) = msb(self.prio_bitmap) {
			return self.pop_from_queue(i as usize);
		}

		None
	}

	/// Remove a specific task handle from the priority queue.
	pub fn remove(&mut self, task: TaskHandle) {
		let queue_index = task.priority.into() as usize;
		//assert!(queue_index < NO_PRIORITIES, "Priority {} is too high", queue_index);

		if let Some(queue) = &mut self.queues[queue_index] {
			let mut i = 0;
			while i != queue.len() {
				if queue[i].id == task.id {
					queue.remove(i);
				} else {
					i += 1;
				}
			}

			if queue.is_empty() {
				self.prio_bitmap &= !(1 << queue_index as u64);
			}
		}
	}
}

struct QueueHead {
	head: Option<Rc<RefCell<Task>>>,
	tail: Option<Rc<RefCell<Task>>>,
}

impl QueueHead {
	pub const fn new() -> Self {
		Self {
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
		const QUEUE_HEAD: QueueHead = QueueHead::new();
		PriorityTaskQueue {
			queues: [QUEUE_HEAD; NO_PRIORITIES],
			prio_bitmap: 0,
		}
	}

	/// Add a task by its priority to the queue
	pub fn push(&mut self, task: Rc<RefCell<Task>>) {
		let i = task.borrow().prio.into() as usize;
		//assert!(i < NO_PRIORITIES, "Priority {} is too high", i);

		self.prio_bitmap |= (1 << i) as u64;
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

		self.queues[i].tail = Some(task);
	}

	fn pop_from_queue(&mut self, queue_index: usize) -> Option<Rc<RefCell<Task>>> {
		let (new_head, task) = {
			let head = self.queues[queue_index].head.as_mut()?;
			let mut borrow = head.borrow_mut();

			if let Some(ref mut nhead) = borrow.next {
				nhead.borrow_mut().prev = None;
			}

			let new_head = borrow.next.clone();
			borrow.next = None;
			borrow.prev = None;

			let task = head.clone();

			(new_head, task)
		};

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
			if i >= prio.into().try_into().unwrap() {
				return self.pop_from_queue(i as usize);
			}
		}

		None
	}

	/// Returns the highest priority of all available task
	pub fn get_highest_priority(&self) -> Priority {
		if let Some(i) = msb(self.prio_bitmap) {
			Priority::from(i.try_into().unwrap())
		} else {
			IDLE_PRIO
		}
	}
}

/// A task control block, which identifies either a process or a thread
#[cfg_attr(any(target_arch = "x86_64", target_arch = "aarch64"), repr(align(128)))]
#[cfg_attr(
	not(any(target_arch = "x86_64", target_arch = "aarch64")),
	repr(align(64))
)]
pub struct Task {
	/// The ID of this context
	pub id: TaskId,
	/// Status of a task, e.g. if the task is ready or blocked
	pub status: TaskStatus,
	/// Task priority,
	pub prio: Priority,
	/// Last stack pointer before a context switch to another task
	pub last_stack_pointer: VirtAddr,
	/// Last stack pointer on the user stack before jumping to kernel space
	pub user_stack_pointer: VirtAddr,
	/// Last FPU state before a context switch to another task using the FPU
	pub last_fpu_state: arch::processor::FPUState,
	/// ID of the core this task is running on
	pub core_id: CoreId,
	/// Stack of the task
	pub stacks: TaskStacks,
	/// next task in queue
	pub next: Option<Rc<RefCell<Task>>>,
	/// previous task in queue
	pub prev: Option<Rc<RefCell<Task>>>,
	/// Task Thread-Local-Storage (TLS)
	pub tls: Option<TaskTLS>,
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
	pub fn new(
		tid: TaskId,
		core_id: CoreId,
		task_status: TaskStatus,
		task_prio: Priority,
		stack_size: usize,
	) -> Task {
		debug!("Creating new task {} on core {}", tid, core_id);

		Task {
			id: tid,
			status: task_status,
			prio: task_prio,
			last_stack_pointer: VirtAddr(0u64),
			user_stack_pointer: VirtAddr(0u64),
			last_fpu_state: arch::processor::FPUState::new(),
			core_id,
			stacks: TaskStacks::new(stack_size),
			next: None,
			prev: None,
			tls: None,
			last_wakeup_reason: WakeupReason::Custom,
			#[cfg(feature = "newlib")]
			lwip_errno: 0,
		}
	}

	pub fn new_idle(tid: TaskId, core_id: CoreId) -> Task {
		debug!("Creating idle task {}", tid);

		Task {
			id: tid,
			status: TaskStatus::Idle,
			prio: IDLE_PRIO,
			last_stack_pointer: VirtAddr(0u64),
			user_stack_pointer: VirtAddr(0u64),
			last_fpu_state: arch::processor::FPUState::new(),
			core_id,
			stacks: TaskStacks::from_boot_stacks(),
			next: None,
			prev: None,
			tls: None,
			last_wakeup_reason: WakeupReason::Custom,
			#[cfg(feature = "newlib")]
			lwip_errno: 0,
		}
	}

	pub fn clone(tid: TaskId, core_id: CoreId, task: &Task) -> Task {
		debug!("Cloning task {} from task {}", tid, task.id);

		Task {
			id: tid,
			status: TaskStatus::Ready,
			prio: task.prio,
			last_stack_pointer: VirtAddr(0u64),
			user_stack_pointer: VirtAddr(0u64),
			last_fpu_state: arch::processor::FPUState::new(),
			core_id,
			stacks: task.stacks.clone(),
			next: None,
			prev: None,
			tls: task.tls.clone(),
			last_wakeup_reason: task.last_wakeup_reason,
			#[cfg(feature = "newlib")]
			lwip_errno: 0,
		}
	}
}

/*impl Drop for Task {
	fn drop(&mut self) {
		debug!("Drop task {}", self.id);
	}
}*/

struct BlockedTask {
	task: Rc<RefCell<Task>>,
	wakeup_time: Option<u64>,
}

impl BlockedTask {
	pub fn new(task: Rc<RefCell<Task>>, wakeup_time: Option<u64>) -> Self {
		Self { task, wakeup_time }
	}
}

pub struct BlockedTaskQueue {
	list: LinkedList<BlockedTask>,
}

impl BlockedTaskQueue {
	pub const fn new() -> Self {
		Self {
			list: LinkedList::new(),
		}
	}

	fn wakeup_task(task: Rc<RefCell<Task>>, reason: WakeupReason) {
		{
			let mut borrowed = task.borrow_mut();
			debug!(
				"Waking up task {} on core {}",
				borrowed.id, borrowed.core_id
			);

			assert!(
				borrowed.core_id == core_id(),
				"Try to wake up task {} on the wrong core {} != {}",
				borrowed.id,
				borrowed.core_id,
				core_id()
			);

			assert!(
				borrowed.status == TaskStatus::Blocked,
				"Trying to wake up task {} which is not blocked",
				borrowed.id
			);
			borrowed.status = TaskStatus::Ready;
			borrowed.last_wakeup_reason = reason;
		}

		// Add the task to the ready queue.
		core_scheduler().ready_queue.push(task);
	}

	/// Blocks the given task for `wakeup_time` ticks, or indefinitely if None is given.
	pub fn add(&mut self, task: Rc<RefCell<Task>>, wakeup_time: Option<u64>) {
		{
			// Set the task status to Blocked.
			let mut borrowed = task.borrow_mut();
			debug!("Blocking task {}", borrowed.id);

			assert_eq!(
				borrowed.status,
				TaskStatus::Running,
				"Trying to block task {} which is not running",
				borrowed.id
			);
			borrowed.status = TaskStatus::Blocked;
		}

		let new_node = BlockedTask::new(task, wakeup_time);

		// Shall the task automatically be woken up after a certain time?
		if let Some(wt) = wakeup_time {
			let first_task = true;
			let mut cursor = self.list.cursor_front_mut();
			let mut _guard = scopeguard::guard(first_task, |first_task| {
				// If the task is the new first task in the list, update the one-shot timer
				// to fire when this task shall be woken up.
				if first_task {
					arch::set_oneshot_timer(wakeup_time);
				}
			});

			while let Some(node) = cursor.current() {
				let node_wakeup_time = node.wakeup_time;
				if node_wakeup_time.is_none() || wt < node_wakeup_time.unwrap() {
					cursor.insert_before(new_node);

					return;
				}

				cursor.move_next();
			}

			// No, then just insert it at the end of the list.
			self.list.push_back(new_node);
		} else {
			// No, then just insert it at the end of the list.
			self.list.push_back(new_node);
		}
	}

	/// Manually wake up a blocked task.
	pub fn custom_wakeup(&mut self, task: TaskHandle) {
		let mut first_task = true;
		let mut cursor = self.list.cursor_front_mut();

		// Loop through all blocked tasks to find it.
		while let Some(node) = cursor.current() {
			if node.task.borrow().id == task.get_id() {
				// Remove it from the list of blocked tasks and wake it up.
				Self::wakeup_task(node.task.clone(), WakeupReason::Custom);
				cursor.remove_current();

				// If this is the first task, adjust the One-Shot Timer to fire at the
				// next task's wakeup time (if any).
				if first_task {
					if let Some(next_node) = cursor.current() {
						arch::set_oneshot_timer(next_node.wakeup_time);
					} else {
						// if no task is available, we have to disable the timer
						arch::set_oneshot_timer(None);
					}
				}

				break;
			}

			first_task = false;
			cursor.move_next();
		}
	}

	/// Wakes up all tasks whose wakeup time has elapsed.
	///
	/// Should be called by the One-Shot Timer interrupt handler when the wakeup time for
	/// at least one task has elapsed.
	pub fn handle_waiting_tasks(&mut self) {
		// Get the current time.
		let time = arch::processor::get_timer_ticks();
		let mut cursor = self.list.cursor_front_mut();

		// Loop through all blocked tasks.
		while let Some(node) = cursor.current() {
			// Get the wakeup time of this task and check if we have reached the first task
			// that hasn't elapsed yet or waits indefinitely.
			let node_wakeup_time = node.wakeup_time;
			if node_wakeup_time.is_none() || time < node_wakeup_time.unwrap() {
				// Adjust the One-Shot Timer to fire at this task's wakeup time (if any)
				// and exit the loop.
				arch::set_oneshot_timer(node_wakeup_time);
				break;
			}

			// Otherwise, this task has elapsed, so remove it from the list and wake it up.
			Self::wakeup_task(node.task.clone(), WakeupReason::Timer);
			cursor.remove_current();
		}
	}
}
