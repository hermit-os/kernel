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

use alloc::rc::Rc;
use arch;
use arch::mm::paging::{BasePageSize, PageSize};
use arch::processor::lsb;
use consts::*;
use core::cell::RefCell;
use core::cmp::Ordering;
use core::{fmt, mem};
use mm;


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

pub const REALTIME_PRIO: Priority = Priority::from(0);
pub const HIGH_PRIO: Priority = Priority::from(0);
pub const NORMAL_PRIO: Priority = Priority::from(24);
pub const LOW_PRIO: Priority = Priority::from(NO_PRIORITIES as u8 - 1);

#[repr(align(64))]
pub struct KernelStack {
	buffer: [u8; KERNEL_STACK_SIZE]
}

impl KernelStack {
	pub const fn new() -> KernelStack {
		KernelStack {
			buffer: [0; KERNEL_STACK_SIZE]
		}
	}

	pub fn top(&self) -> usize {
		(&(self.buffer[KERNEL_STACK_SIZE - 1]) as *const _) as usize
	}

	pub fn bottom(&self) -> usize {
		(&(self.buffer[0]) as *const _) as usize
	}
}

/// The stack is too large to use the default debug trait. => create our own.
impl fmt::Debug for KernelStack {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		for x in self.buffer.iter() {
			write!(f, "{:X}", x)?;
		}

		Ok(())
	}
}

/// Simple task queue, which doesn't need any allocation of memory
#[derive(Default)]
pub struct TaskQueue {
	head: Option<Rc<RefCell<Task>>>,
	tail: Option<Rc<RefCell<Task>>>
}

impl TaskQueue {
	/// Creates an empty task queue
	pub const fn new() -> TaskQueue {
		TaskQueue {
			head: None,
			tail: None
		}
	}

	/// Check if the queue is empty
	#[inline(always)]
	pub fn is_empty(&self) -> bool {
		self.head.is_none()
	}

	/// Add task at the end of the queue
	pub fn push_back(&mut self, new_task: Rc<RefCell<Task>>) {
		{
			let mut new_task_borrowed = new_task.borrow_mut();
			new_task_borrowed.next = None;

			// Check if we already have any tasks in the list.
			match self.tail.take() {
				Some(tail) => {
					// We become the next task of the old list tail and the old list tail becomes our previous task.
					tail.borrow_mut().next = Some(new_task.clone());
				},
				None => {
					// No tasks yet, so we become the new list head.
					self.head = Some(new_task.clone());
				}
			}
		}

		// In any case, we become the new list tail.
		self.tail = Some(new_task);
	}

	/// Pop the first task of the queue
	pub fn pop_front(&mut self) -> Option<Rc<RefCell<Task>>> {
		if let Some(head) = self.head.take() {
			match head.borrow_mut().next.take() {
				Some(next_task) => self.head = Some(next_task),
				None => self.tail = None
			};
			Some(head)
		} else {
			None
		}
	}
}

/// Realize a priority queue for tasks
pub struct PriorityTaskQueue {
	queues: [TaskQueue; NO_PRIORITIES],
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

	/// Add task by its priority to the queue
	pub fn push(&mut self, prio: Priority, task: Rc<RefCell<Task>>) {
		let mut i = prio.into() as usize;

		if i >= NO_PRIORITIES {
			info!("priority with {} is too high for TaskQueue::push()!", prio);
			i = NO_PRIORITIES - 1;
		}

		self.prio_bitmap |= 1 << i;
		self.queues[i].push_back(task);
	}

	/// Pop the task with the highest priority from the queue
	pub fn pop(&mut self) -> Option<Rc<RefCell<Task>>> {
		let i = lsb(self.prio_bitmap);

		if i < NO_PRIORITIES as u64 {
			let ret = self.queues[i as usize].pop_front();

			if self.queues[i as usize].is_empty() {
				self.prio_bitmap &= !(1 << i);
			}

			ret
		} else {
			None
		}
	}

	/// Pop the next task, which has a higher or the same priority as `prio`
	pub fn pop_with_prio(&mut self, prio: Priority) -> Option<Rc<RefCell<Task>>> {
		let i = lsb(self.prio_bitmap);

		if i <= prio.into() as u64 {
			let ret = self.queues[i as usize].pop_front();

			if self.queues[i as usize].is_empty() == true {
				self.prio_bitmap &= !(1 << i);
			}

			ret
		} else {
			None
		}
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
			address: mm::allocate(memory_size),
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
	/// points to the next task within a task queue
	next: Option<Rc<RefCell<Task>>>,
	/// Stack of the task
	pub stack: *mut KernelStack,
	/// Stack for interrupt handling
	pub ist: *mut KernelStack,
	/// Task heap area
	pub heap: Option<TaskHeap>,
	/// Task Thread-Local-Storage (TLS)
	pub tls: Option<TaskTLS>
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
			mm::deallocate(self.stack as usize, mem::size_of::<KernelStack>());
			mm::deallocate(self.ist as usize, mem::size_of::<KernelStack>());
		}
	}
}

impl Task {
	pub fn new(tid: TaskId, core_id: u32, task_status: TaskStatus, task_prio: Priority, heap_start: Option<usize>) -> Task {
		let stack = mm::allocate(mem::size_of::<KernelStack>()) as *mut KernelStack;
		let ist = mm::allocate(mem::size_of::<KernelStack>()) as *mut KernelStack;
		debug!("Allocating stack {:#X} and IST {:#X} for task {}", stack as usize, ist as usize, tid);

		Task {
			id: tid,
			status: task_status,
			prio: task_prio,
			last_stack_pointer: 0,
			last_fpu_state: arch::processor::FPUState::new(),
			core_id: core_id,
			next: None,
			stack: stack,
			ist: ist,
			heap: heap_start.map(|start| TaskHeap { start: start, end: start }),
			tls: None,
		}
	}

	pub fn new_idle(tid: TaskId, core_id: u32) -> Task {
		let (stack, ist) = arch::get_boot_stacks();
		Task {
			id: tid,
			status: TaskStatus::TaskIdle,
			prio: LOW_PRIO,
			last_stack_pointer: 0,
			last_fpu_state: arch::processor::FPUState::new(),
			core_id: core_id,
			next: None,
			stack: stack as *mut KernelStack,
			ist: ist as *mut KernelStack,
			heap: None,
			tls: None,
		}
	}
}

/// Struct to sort the tasks by wakeup time
pub struct WaitingTask {
	pub wakeup_time: usize,
	pub task: Rc<RefCell<Task>>,
}

impl WaitingTask {
	pub fn new(t: Rc<RefCell<Task>>, wt: usize) -> WaitingTask {
		WaitingTask {
			wakeup_time: wt,
			task: t
		}
	}
}

impl Eq for WaitingTask {}

impl PartialOrd for WaitingTask {
    fn partial_cmp(&self, other: &WaitingTask) -> Option<Ordering> {
        Some(self.wakeup_time.cmp(&other.wakeup_time).reverse())
    }
}

impl Ord for WaitingTask {
    fn cmp(&self, other: &WaitingTask) -> Ordering {
        self.wakeup_time.cmp(&other.wakeup_time).reverse()
    }
}

impl PartialEq for WaitingTask {
    fn eq(&self, other: &WaitingTask) -> bool {
        self.wakeup_time == other.wakeup_time
    }
}
