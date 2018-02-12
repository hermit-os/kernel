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

include!(concat!(env!("CARGO_TARGET_DIR"), "/config.rs"));

use alloc::rc::Rc;
use arch;
use arch::mm::paging::{BasePageSize, PageSize, PageTableEntryFlags};
use arch::processor::msb;
use collections::{DoublyLinkedList, Node};
use core::cell::RefCell;
use core::fmt;
use mm;
use scheduler;
use spin::RwLock;


/// The status of the task - used for scheduling
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TaskStatus {
	TaskInvalid,
	TaskReady,
	TaskRunning,
	TaskBlocked,
	TaskFinished,
	TaskIdle
}

/// Reason why wakeup() has been called on a task.
#[derive(Clone, Copy, PartialEq)]
pub enum WakeupReason {
	Custom,
	Timer,
}

/// Unique identifier for a task (i.e. `pid`).
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Copy)]
pub struct TaskId(usize);

impl TaskId {
	pub const fn into(self) -> usize {
		self.0
	}

	pub const fn from(x: usize) -> Self {
		TaskId(x)
	}
}

impl fmt::Display for TaskId {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
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
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{}", self.0)
	}
}

pub const HIGH_PRIO: Priority = Priority::from(3);
pub const NORMAL_PRIO: Priority = Priority::from(2);
pub const LOW_PRIO: Priority = Priority::from(1);
pub const IDLE_PRIO: Priority = Priority::from(0);

/// Maximum number of priorities
pub const NO_PRIORITIES: usize = 4;

/// Realize a priority queue for tasks
pub struct PriorityTaskQueue {
	queues: [DoublyLinkedList<Rc<RefCell<Task>>>; NO_PRIORITIES],
	prio_bitmap: u64
}

impl PriorityTaskQueue {
	/// Creates an empty priority queue for tasks
	pub fn new() -> PriorityTaskQueue {
		PriorityTaskQueue {
			queues: Default::default(),
			prio_bitmap: 0
		}
	}

	/// Add a task by its priority to the queue
	pub fn push(&mut self, prio: Priority, task: Rc<RefCell<Task>>) {
		let i = prio.into() as usize;
		assert!(i < NO_PRIORITIES, "Priority {} is too high", i);

		self.prio_bitmap |= 1 << i;
		self.queues[i].push(Node::new(task));
	}

	fn pop_from_queue(&mut self, queue_index: usize) -> Option<Rc<RefCell<Task>>> {
		let first_task = self.queues[queue_index].head();
		first_task.map(|task| {
			self.queues[queue_index].remove(task.clone());

			if self.queues[queue_index].head().is_none() {
				self.prio_bitmap &= !(1 << queue_index as u64);
			}

			task.borrow().value.clone()
		})
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
			if i >= prio.into() as u64 {
				return self.pop_from_queue(i as usize);
			}
		}

		None
	}
}


pub struct TaskHeap {
	pub start: usize,
	pub end: usize,
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
			address: mm::allocate(memory_size, PageTableEntryFlags::EXECUTE_DISABLE),
			size: memory_size
		}
	}

	pub fn address(&self) -> usize {
		self.address
	}
}

impl Drop for TaskTLS {
	fn drop(&mut self) {
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
	pub core_id: u32,
	/// Stack of the task
	pub stack: usize,
	/// Stack for interrupt handling
	pub ist: usize,
	/// Task heap area
	pub heap: Option<Rc<RefCell<RwLock<TaskHeap>>>>,
	/// Task Thread-Local-Storage (TLS)
	pub tls: Option<Rc<RefCell<TaskTLS>>>,
	/// Reason why wakeup() has been called the last time
	pub last_wakeup_reason: WakeupReason,
	/// lwIP error code for this task
	pub lwip_errno: i32,
}

pub trait TaskFrame {
	/// Create the initial stack frame for a new task
	fn create_stack_frame(&mut self, func: extern "C" fn(usize), arg: usize);
}

impl Drop for Task {
    fn drop(&mut self) {
		if self.status != TaskStatus::TaskIdle {
			debug!("Deallocating stack {:#X} and IST {:#X} for task {}", self.stack as usize, self.ist as usize, self.id);

			// deallocate stacks
			mm::deallocate(self.stack as usize, DEFAULT_STACK_SIZE);
			mm::deallocate(self.ist as usize, DEFAULT_STACK_SIZE);
		}
	}
}

impl Task {
	#[inline]
	fn allocate_stacks() -> (usize, usize) {
		// Allocate an executable stack to possibly support dynamically generated code on the stack (see https://security.stackexchange.com/a/47825).
		let stack = mm::allocate(DEFAULT_STACK_SIZE, PageTableEntryFlags::empty());
		let ist = mm::allocate(KERNEL_STACK_SIZE, PageTableEntryFlags::EXECUTE_DISABLE);
		(stack, ist)
	}

	pub fn new(tid: TaskId, core_id: u32, task_status: TaskStatus, task_prio: Priority, heap_start: Option<usize>) -> Task {
		let (stack, ist) = Task::allocate_stacks();
		debug!("Allocating stack {:#X} and IST {:#X} for task {}", stack, ist, tid);

		Task {
			id: tid,
			status: task_status,
			prio: task_prio,
			last_stack_pointer: 0,
			last_fpu_state: arch::processor::FPUState::new(),
			core_id: core_id,
			stack: stack,
			ist: ist,
			heap: heap_start.map(|start| Rc::new(RefCell::new(RwLock::new(TaskHeap { start: start, end: start })))),
			tls: None,
			last_wakeup_reason: WakeupReason::Custom,
			lwip_errno: 0,
		}
	}

	pub fn new_idle(tid: TaskId, core_id: u32) -> Task {
		let (stack, ist) = arch::get_boot_stacks();
		debug!("Using boot stack {:#X} and IST {:#X} for idle task {}", stack, ist, tid);

		Task {
			id: tid,
			status: TaskStatus::TaskIdle,
			prio: IDLE_PRIO,
			last_stack_pointer: 0,
			last_fpu_state: arch::processor::FPUState::new(),
			core_id: core_id,
			stack: stack,
			ist: ist,
			heap: None,
			tls: None,
			last_wakeup_reason: WakeupReason::Custom,
			lwip_errno: 0,
		}
	}

	pub fn clone(tid: TaskId, core_id: u32, task: &Task) -> Task {
		let (stack, ist) = Task::allocate_stacks();
		debug!("Allocating stack {:#X} and IST {:#X} for task {} cloned from task {}", stack, ist, tid, task.id);

		Task {
			id: tid,
			status: TaskStatus::TaskReady,
			prio: task.prio,
			last_stack_pointer: 0,
			last_fpu_state: arch::processor::FPUState::new(),
			core_id: core_id,
			stack: stack,
			ist: ist,
			heap: task.heap.clone(),
			tls: task.tls.clone(),
			last_wakeup_reason: task.last_wakeup_reason,
			lwip_errno: 0,
		}
	}
}

struct BlockedTask {
	task: Rc<RefCell<Task>>,
	wakeup_time: Option<usize>,
}

pub struct BlockedTaskQueue {
	list: DoublyLinkedList<BlockedTask>
}

impl BlockedTaskQueue {
	pub const fn new() -> Self {
		Self { list: DoublyLinkedList::new() }
	}

	fn wakeup_task(task: Rc<RefCell<Task>>, reason: WakeupReason) {
		let (core_id, prio) = {
			let mut borrowed = task.borrow_mut();
			info!("Waking up task {} on core {}", borrowed.id, borrowed.core_id);

			assert!(borrowed.status == TaskStatus::TaskBlocked, "Trying to wake up task {} which is not blocked", borrowed.id);
			borrowed.status = TaskStatus::TaskReady;
			borrowed.last_wakeup_reason = reason;

			(borrowed.core_id, borrowed.prio)
		};

		let core_scheduler = scheduler::get_scheduler(core_id);
		core_scheduler.ready_queue.lock().push(prio, task);

		// If that CPU has been running the Idle task, it may be in a HALT state and needs to be woken up.
		let task_locked = core_scheduler.current_task.read();
		if task_locked.borrow().status == TaskStatus::TaskIdle {
			arch::wakeup_core(core_id);
		}
	}

	/// Blocks the given task for `wakeup_time` ticks, or indefinitely if None is given.
	pub fn add(&mut self, task: Rc<RefCell<Task>>, wakeup_time: Option<usize>) {
		{
			// Set the task status to Blocked.
			let mut borrowed = task.borrow_mut();
			info!("Blocking task {}", borrowed.id);

			assert!(borrowed.status == TaskStatus::TaskRunning, "Trying to block task {} which is not running", borrowed.id);
			borrowed.status = TaskStatus::TaskBlocked;
		}

		let new_node = Node::new(BlockedTask { task: task, wakeup_time: wakeup_time });

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
		let time = arch::processor::update_timer_ticks();

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
