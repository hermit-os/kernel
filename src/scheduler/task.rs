use alloc::boxed::Box;
use alloc::collections::{LinkedList, VecDeque};
use alloc::rc::Rc;
use core::cell::RefCell;
use core::cmp::Ordering;
use core::fmt;
use core::num::NonZeroU64;
#[cfg(any(feature = "tcp", feature = "udp"))]
use core::ops::DerefMut;

use crate::arch;
use crate::arch::core_local::*;
use crate::arch::mm::VirtAddr;
use crate::arch::scheduler::{TaskStacks, TaskTLS};
use crate::scheduler::CoreId;

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
	#[cfg(feature = "smp")]
	core_id: CoreId,
}

impl TaskHandle {
	pub fn new(id: TaskId, priority: Priority, #[cfg(feature = "smp")] core_id: CoreId) -> Self {
		Self {
			id,
			priority,
			#[cfg(feature = "smp")]
			core_id,
		}
	}

	#[cfg(feature = "smp")]
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
#[derive(Default)]
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

	/// Checks if the queue is empty.
	pub fn is_empty(&self) -> bool {
		self.prio_bitmap == 0
	}

	/// Checks if the given task is in the queue. Returns `true` if the task
	/// was found.
	pub fn contains(&self, task: TaskHandle) -> bool {
		matches!(self.queues[task.priority.into() as usize]
			.as_ref(), Some(queue) if queue.iter().any(|queued| queued.id == task.id))
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

	/// Remove a specific task handle from the priority queue. Returns `true` if
	/// the handle was in the queue.
	pub fn remove(&mut self, task: TaskHandle) -> bool {
		let queue_index = task.priority.into() as usize;
		//assert!(queue_index < NO_PRIORITIES, "Priority {} is too high", queue_index);

		let mut success = false;
		if let Some(queue) = &mut self.queues[queue_index] {
			let mut i = 0;
			while i != queue.len() {
				if queue[i].id == task.id {
					queue.remove(i);
					success = true;
				} else {
					i += 1;
				}
			}

			if queue.is_empty() {
				self.prio_bitmap &= !(1 << queue_index as u64);
			}
		}

		success
	}
}

/// Realize a priority queue for tasks
pub struct PriorityTaskQueue {
	queues: [LinkedList<Rc<RefCell<Task>>>; NO_PRIORITIES],
	prio_bitmap: u64,
}

impl PriorityTaskQueue {
	/// Creates an empty priority queue for tasks
	pub const fn new() -> PriorityTaskQueue {
		const EMPTY_LIST: LinkedList<Rc<RefCell<Task>>> = LinkedList::new();
		PriorityTaskQueue {
			queues: [EMPTY_LIST; NO_PRIORITIES],
			prio_bitmap: 0,
		}
	}

	/// Add a task by its priority to the queue
	pub fn push(&mut self, task: Rc<RefCell<Task>>) {
		let i = task.borrow().prio.into() as usize;
		//assert!(i < NO_PRIORITIES, "Priority {} is too high", i);

		self.prio_bitmap |= (1 << i) as u64;
		let queue = &mut self.queues[i];
		queue.push_back(task);
	}

	fn pop_from_queue(&mut self, queue_index: usize) -> Option<Rc<RefCell<Task>>> {
		let task = self.queues[queue_index].pop_front();
		if self.queues[queue_index].is_empty() {
			self.prio_bitmap &= !(1 << queue_index as u64);
		}

		task
	}

	/// Remove the task at index from the queue and return that task,
	/// or None if the index is out of range or the list is empty.
	fn remove_from_queue(
		&mut self,
		task_index: usize,
		queue_index: usize,
	) -> Option<Rc<RefCell<Task>>> {
		//assert!(prio < NO_PRIORITIES, "Priority {} is too high", prio);

		let queue = &mut self.queues[queue_index];
		if task_index <= queue.len() {
			// Calling remove is unstable: https://github.com/rust-lang/rust/issues/69210
			let mut split_list = queue.split_off(task_index);
			let element = split_list.pop_front();
			queue.append(&mut split_list);
			if queue.is_empty() {
				self.prio_bitmap &= !(1 << queue_index as u64);
			}
			element
		} else {
			None
		}
	}

	/// Returns true if the queue is empty.
	pub fn is_empty(&self) -> bool {
		self.prio_bitmap == 0
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
	#[cfg(all(target_arch = "x86_64", feature = "smp"))]
	pub fn get_highest_priority(&self) -> Priority {
		if let Some(i) = msb(self.prio_bitmap) {
			Priority::from(i.try_into().unwrap())
		} else {
			IDLE_PRIO
		}
	}

	/// Change priority of specific task
	pub fn set_priority(&mut self, handle: TaskHandle, prio: Priority) -> Result<(), ()> {
		let old_priority = handle.get_priority().into() as usize;
		if let Some(index) = self.queues[old_priority]
			.iter()
			.position(|current_task| current_task.borrow().id == handle.id)
		{
			let Some(task) = self.remove_from_queue(index, old_priority) else {
				return Err(());
			};
			task.borrow_mut().prio = prio;
			self.push(task);
			return Ok(());
		}

		Err(())
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
	/// Task Thread-Local-Storage (TLS)
	pub tls: Option<Box<TaskTLS>>,
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
		stacks: TaskStacks,
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
			stacks,
			tls: None,
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
			tls: None,
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
	#[cfg(any(feature = "tcp", feature = "udp"))]
	network_wakeup_time: Option<u64>,
}

impl BlockedTaskQueue {
	pub const fn new() -> Self {
		Self {
			list: LinkedList::new(),
			#[cfg(any(feature = "tcp", feature = "udp"))]
			network_wakeup_time: None,
		}
	}

	fn wakeup_task(task: Rc<RefCell<Task>>) {
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
		}

		// Add the task to the ready queue.
		#[cfg(target_os = "none")]
		core_scheduler().ready_queue.push(task);
	}

	#[cfg(any(feature = "tcp", feature = "udp"))]
	pub fn add_network_timer(&mut self, wakeup_time: Option<u64>) {
		self.network_wakeup_time = wakeup_time;

		let next = self.list.front().and_then(|t| t.wakeup_time);

		let time = match (wakeup_time, next) {
			(Some(a), Some(b)) => Some(a.min(b)),
			(a, b) => a.or(b),
		};

		arch::set_oneshot_timer(time);
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
			let mut cursor = self.list.cursor_front_mut();
			let set_oneshot_timer = || {
				#[cfg(not(any(feature = "tcp", feature = "udp")))]
				arch::set_oneshot_timer(wakeup_time);
				#[cfg(any(feature = "tcp", feature = "udp"))]
				match self.network_wakeup_time {
					Some(time) => {
						if time > wt {
							arch::set_oneshot_timer(wakeup_time);
						} else {
							arch::set_oneshot_timer(self.network_wakeup_time);
						}
					}
					_ => arch::set_oneshot_timer(wakeup_time),
				}
			};

			while let Some(node) = cursor.current() {
				let node_wakeup_time = node.wakeup_time;
				if node_wakeup_time.is_none() || wt < node_wakeup_time.unwrap() {
					cursor.insert_before(new_node);

					set_oneshot_timer();
					return;
				}

				cursor.move_next();
			}

			set_oneshot_timer();
		}

		self.list.push_back(new_node);
	}

	/// Manually wake up a blocked task.
	pub fn custom_wakeup(&mut self, task: TaskHandle) {
		let mut first_task = true;
		let mut cursor = self.list.cursor_front_mut();

		#[cfg(any(feature = "tcp", feature = "udp"))]
		if let Some(wakeup_time) = self.network_wakeup_time {
			if wakeup_time <= arch::processor::get_timer_ticks() {
				self.network_wakeup_time = None;
			}
		}

		// Loop through all blocked tasks to find it.
		while let Some(node) = cursor.current() {
			if node.task.borrow().id == task.get_id() {
				// Remove it from the list of blocked tasks and wake it up.
				Self::wakeup_task(node.task.clone());
				cursor.remove_current();

				// If this is the first task, adjust the One-Shot Timer to fire at the
				// next task's wakeup time (if any).
				#[cfg(any(feature = "tcp", feature = "udp"))]
				if first_task {
					arch::set_oneshot_timer(cursor.current().map_or_else(
						|| self.network_wakeup_time,
						|node| match node.wakeup_time {
							Some(wt) => {
								if let Some(timer) = self.network_wakeup_time {
									if wt < timer {
										Some(wt)
									} else {
										Some(timer)
									}
								} else {
									Some(wt)
								}
							}
							None => self.network_wakeup_time,
						},
					));
				}
				#[cfg(not(any(feature = "tcp", feature = "udp")))]
				if first_task {
					arch::set_oneshot_timer(
						cursor
							.current()
							.map_or_else(|| None, |node| node.wakeup_time),
					);
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

		#[cfg(any(feature = "tcp", feature = "udp"))]
		if let Some(mut guard) = crate::executor::network::NIC.try_lock() {
			if let crate::executor::network::NetworkState::Initialized(nic) = guard.deref_mut() {
				let now = crate::executor::network::now();
				nic.poll_common(now);
				self.network_wakeup_time = nic.poll_delay(now).map(|d| d.total_micros() + time);
			}
		}

		// Loop through all blocked tasks.
		let mut cursor = self.list.cursor_front_mut();
		while let Some(node) = cursor.current() {
			// Get the wakeup time of this task and check if we have reached the first task
			// that hasn't elapsed yet or waits indefinitely.
			let node_wakeup_time = node.wakeup_time;
			if node_wakeup_time.is_none() || time < node_wakeup_time.unwrap() {
				break;
			}

			// Otherwise, this task has elapsed, so remove it from the list and wake it up.
			Self::wakeup_task(node.task.clone());
			cursor.remove_current();
		}

		#[cfg(any(feature = "tcp", feature = "udp"))]
		arch::set_oneshot_timer(cursor.current().map_or_else(
			|| self.network_wakeup_time,
			|node| match node.wakeup_time {
				Some(wt) => {
					if let Some(timer) = self.network_wakeup_time {
						if wt < timer {
							Some(wt)
						} else {
							Some(timer)
						}
					} else {
						Some(wt)
					}
				}
				None => self.network_wakeup_time,
			},
		));
		#[cfg(not(any(feature = "tcp", feature = "udp")))]
		arch::set_oneshot_timer(
			cursor
				.current()
				.map_or_else(|| None, |node| node.wakeup_time),
		);
	}
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {

	use super::*;
	use crate::arch::scheduler::CommonStack;

	#[test]
	fn test_TaskQueueEmpty() {
		let PrioQueue = PriorityTaskQueue::new();
		assert!(PrioQueue.is_empty());
	}

	#[test]
	fn test_TaskQueuePush() {
		let mut task_queue = PriorityTaskQueue::new();
		let mut tasks: Vec<Rc<RefCell<Task>>> = Vec::new();
		struct PAddr(pub u64);
		let mut queue_length = 1;
		let mut counter = 0;

		// create some tasks with different priorities and push them in a vector
		for i in 0..11 {
			let task = Rc::new(RefCell::new(Task {
				id: TaskId(i),
				status: TaskStatus::Running,
				prio: Priority(i as u8),
				last_stack_pointer: x86::bits64::paging::VAddr(10),
				user_stack_pointer: x86::bits64::paging::VAddr(10),
				last_fpu_state: arch::processor::FPUState::new(),
				core_id: 1,
				stacks: TaskStacks::Common(CommonStack {
					virt_addr: x86::bits64::paging::VAddr::from_u64(2000),
					phys_addr: x86::bits64::paging::PAddr(2000),
					total_size: 4000,
				}),
				tls: None,
				#[cfg(feature = "newlib")]
				lwip_errno: 0,
			}));
			// two tasks with same priority (Task id 9 and 10)
			if i == 10 {
				task.borrow_mut().prio = Priority((i - 1) as u8);
			}
			// push tasks in vector (will be iterated over the vector in the next lines)
			tasks.push(task);
		}
		// push the tasks in the queue
		for task in tasks {
			task_queue.push(task.clone());
			let i = task.borrow().prio.into() as usize;
			// verifying Queue bitmap
			assert_eq!((task_queue.prio_bitmap & 1 << i), 1 << i);

			if counter == 10 {
				queue_length = 2; // two tasks are added in the same queue
			}

			// verifying queue length
			assert_eq!(task_queue.queues[i].len(), queue_length);
			// verifiying task ids with the ones in the Queue
			if (counter < 10) {
				assert_eq!(
					task_queue.queues[i].front().unwrap().as_ptr(),
					task.as_ptr(),
					"Queue {} should contain task with ID {}",
					i,
					task.borrow().id.0
				);
			} else {
				task_queue.pop(); // remove first element (task id 9)
				assert_eq!(
					task_queue.queues[i].front().unwrap().borrow().id.0,
					task.borrow().id.0
				);
			}
			counter = counter + 1;
		}
	}

	#[test]
	fn test_TaskQueuePop() {
		let mut task_queue = PriorityTaskQueue::new();
		struct PAddr(pub u64);

		// Create 3 Queues with 2 tasks each
		for i in 0..6 {
			let task = Rc::new(RefCell::new(Task {
				id: TaskId(i),
				status: TaskStatus::Running,
				prio: Priority((i % 3) as u8),
				last_stack_pointer: x86::bits64::paging::VAddr(10),
				user_stack_pointer: x86::bits64::paging::VAddr(10),
				last_fpu_state: arch::processor::FPUState::new(),
				core_id: 1,
				stacks: TaskStacks::Common(CommonStack {
					virt_addr: x86::bits64::paging::VAddr::from_u64(2000),
					phys_addr: x86::bits64::paging::PAddr(2000),
					total_size: 4000,
				}),
				tls: None,
				#[cfg(feature = "newlib")]
				lwip_errno: 0,
			}));

			task_queue.push(task.clone());
		}

		for i in 0..3 {
			let prio = 2 - i;
			assert_eq!(task_queue.queues[prio].len(), 2);
			assert_eq!((task_queue.prio_bitmap & 1 << prio), (1 << prio));

			task_queue.pop(); // first pop in the Queue

			// verifying queue length
			assert_eq!(task_queue.queues[prio].len(), 1);
			assert_eq!((task_queue.prio_bitmap & 1 << prio), (1 << prio));

			task_queue.pop(); // second pop in the Queue

			assert!(task_queue.queues[prio].is_empty());
			assert_eq!((task_queue.prio_bitmap & (1 << prio)), 0); // Queue should be empty
		}
	}

	#[test]
	fn test_TaskQueuePopWithPriority() {
		let mut task_queue = PriorityTaskQueue::new();
		struct PAddr(pub u64);

		// Create two tasks with different prios
		for i in 0..2 {
			let task = Rc::new(RefCell::new(Task {
				id: TaskId(i),
				status: TaskStatus::Running,
				prio: Priority((i % 2) as u8),
				last_stack_pointer: x86::bits64::paging::VAddr(10),
				user_stack_pointer: x86::bits64::paging::VAddr(10),
				last_fpu_state: arch::processor::FPUState::new(),
				core_id: 1,
				stacks: TaskStacks::Common(CommonStack {
					virt_addr: x86::bits64::paging::VAddr::from_u64(2000),
					phys_addr: x86::bits64::paging::PAddr(2000),
					total_size: 4000,
				}),
				tls: None,
				#[cfg(feature = "newlib")]
				lwip_errno: 0,
			}));

			task_queue.push(task.clone());
		}

		assert_eq!(task_queue.queues[1].len(), 1);
		assert_eq!((task_queue.prio_bitmap & 1 << 1), (1 << 1));

		task_queue.pop_with_prio(Priority(1 as u8)); // pop task with prio 1

		assert!(task_queue.queues[1].is_empty());
		assert_eq!((task_queue.prio_bitmap & (1 << 1)), 0); // Queue of prio 1 should be empty

		task_queue.pop_with_prio(Priority(1 as u8));

		assert_eq!(task_queue.queues[0].len(), 1);
		assert_eq!((task_queue.prio_bitmap & (1 << 0)), 1);
	}

	#[test]
	fn test_TaskQueueSetPriority() {
		let mut task_queue = PriorityTaskQueue::new();
		struct PAddr(pub u64);

		// create task with priority 0
		let task = Rc::new(RefCell::new(Task {
			id: TaskId(7),
			status: TaskStatus::Running,
			prio: Priority((0) as u8),
			last_stack_pointer: x86::bits64::paging::VAddr(10),
			user_stack_pointer: x86::bits64::paging::VAddr(10),
			last_fpu_state: arch::processor::FPUState::new(),
			core_id: 1,
			stacks: TaskStacks::Common(CommonStack {
				virt_addr: x86::bits64::paging::VAddr::from_u64(2000),
				phys_addr: x86::bits64::paging::PAddr(2000),
				total_size: 4000,
			}),
			tls: None,
			#[cfg(feature = "newlib")]
			lwip_errno: 0,
		}));

		task_queue.push(task.clone());

		// create task handle with same
		let mut task_id = TaskId::from(7);
		let prio = Priority::from(0);
		#[cfg(feature = "smp")]
		let cid: CoreId = 1;

		let task_handle = TaskHandle {
			id: task_id,
			priority: prio,
			#[cfg(feature = "smp")]
			core_id: cid,
		};

		assert_eq!(task_queue.queues[0].len(), 1);
		assert_eq!((task_queue.prio_bitmap & 1 << 0), (1 << 0));

		assert_eq!(task_queue.queues[1].len(), 0);
		assert_eq!((task_queue.prio_bitmap & 1 << 1), 0);

		task_queue.set_priority(task_handle, Priority(1 as u8)); // pop task with prio 1

		assert!(task_queue.queues[0].is_empty());
		assert_eq!((task_queue.prio_bitmap & (1 << 1)), 1 << 1); // Queue of prio 1 should be empty
	}

	#[test]
	fn test_TaskQueueGetHighestPriority() {
		let mut task_queue = PriorityTaskQueue::new();
		struct PAddr(pub u64);

		// empty Queue, should return prio 0
		assert_eq!(task_queue.get_highest_priority(), IDLE_PRIO);

		// Create 3 tasks with different prios
		for i in 0..3 {
			let task = Rc::new(RefCell::new(Task {
				id: TaskId(i),
				status: TaskStatus::Running,
				prio: Priority((i) as u8),
				last_stack_pointer: x86::bits64::paging::VAddr(10),
				user_stack_pointer: x86::bits64::paging::VAddr(10),
				last_fpu_state: arch::processor::FPUState::new(),
				core_id: 1,
				stacks: TaskStacks::Common(CommonStack {
					virt_addr: x86::bits64::paging::VAddr::from_u64(2000),
					phys_addr: x86::bits64::paging::PAddr(2000),
					total_size: 4000,
				}),
				tls: None,
				#[cfg(feature = "newlib")]
				lwip_errno: 0,
			}));

			task_queue.push(task.clone());
		}

		assert_eq!(task_queue.get_highest_priority(), Priority(2 as u8)); // pop task with prio 2
		task_queue.pop();
		assert_eq!(task_queue.get_highest_priority(), Priority(1 as u8)); // pop task with prio 1
	}

	#[test]
	fn test_TaskHandleEqCmp() {
		let mut task_id = TaskId::from(1);
		let prio = Priority::from(5);
		#[cfg(feature = "smp")]
		let cid: CoreId = 0;

		let task_handle = TaskHandle {
			id: task_id,
			priority: prio,
			#[cfg(feature = "smp")]
			core_id: cid,
		};

		let task_handle_from_new = TaskHandle::new(
			task_id,
			prio,
			#[cfg(feature = "smp")]
			cid,
		);
		let result = task_handle_from_new.eq(&task_handle);
		assert_eq!(result, true); //Test eq function. return true

		let mut task_id = TaskId::from(5);
		let task_handle_from_new = TaskHandle::new(
			task_id,
			prio,
			#[cfg(feature = "smp")]
			cid,
		);
		let result = task_handle_from_new.cmp(&task_handle);
		assert_eq!(result, Ordering::Greater); //Test cmp function. return GREATER
	}

	#[test]
	fn test_TaskHandlePushPop() {
		let mut PrioQueue = TaskHandlePriorityQueue::new();
		let mut tasks: Vec<TaskHandle> = Vec::new();
		let cid: CoreId = 0;

		// check if queue is empty
		assert!(PrioQueue.is_empty());

		for i in 0..10 {
			let mut task_id = TaskId::from(i);
			let prio = Priority::from(i as u8);
			#[cfg(feature = "smp")]
			let task_handle = TaskHandle::new(
				task_id,
				prio,
				#[cfg(feature = "smp")]
				cid,
			);
			tasks.push(task_handle);
		}

		// push test
		for task_handle in tasks {
			PrioQueue.push(task_handle);
			let i = task_handle.priority.into() as usize;
			assert_eq!((PrioQueue.prio_bitmap & 1 << i), (1 << i));
		}

		// pop_from_queue test
		assert_eq!(
			PrioQueue.pop_from_queue(9),
			Some(TaskHandle::new(
				TaskId::from(9),
				Priority::from(9 as u8),
				#[cfg(feature = "smp")]
				cid
			))
		);
		assert_eq!((PrioQueue.prio_bitmap & (1 << 9)), 0);

		// pop test
		for idx in 0..9 {
			PrioQueue.pop();
			assert_eq!((PrioQueue.prio_bitmap & (1 << (8 - idx))), 0);
		}
	}

	#[test]
	fn test_TaskHandleContains() {
		let mut PrioQueue = TaskHandlePriorityQueue::new();
		#[cfg(feature = "smp")]
		let cid: CoreId = 0;

		let mut task_id = TaskId::from(0);
		let prio = Priority::from(0 as u8);

		let task_handle = TaskHandle::new(
			task_id,
			prio,
			#[cfg(feature = "smp")]
			cid,
		);
		let mut PrioQueue = TaskHandlePriorityQueue::new();
		PrioQueue.push(task_handle);

		let task_handle_exist = TaskHandle::new(
			TaskId::from(0),
			Priority::from(0 as u8),
			#[cfg(feature = "smp")]
			cid,
		);
		let task_handle_not_exist = TaskHandle::new(
			TaskId::from(1),
			Priority::from(0 as u8),
			#[cfg(feature = "smp")]
			cid,
		);

		assert!(PrioQueue.contains(task_handle_exist));

		assert!(!PrioQueue.contains(task_handle_not_exist));
	}

	#[test]
	fn test_TaskHandleRemove() {
		let mut PrioQueue = TaskHandlePriorityQueue::new();
		#[cfg(feature = "smp")]
		let cid: CoreId = 0;

		let task_handle1 = TaskHandle::new(
			TaskId::from(0),
			Priority::from(1 as u8),
			#[cfg(feature = "smp")]
			cid,
		);
		let task_handle2 = TaskHandle::new(
			TaskId::from(1),
			Priority::from(1 as u8),
			#[cfg(feature = "smp")]
			cid,
		);

		PrioQueue.push(task_handle1);
		PrioQueue.push(task_handle2);

		assert!(PrioQueue.contains(task_handle1));
		assert!(PrioQueue.remove(task_handle1));
		assert!(!PrioQueue.contains(task_handle1));
	}

	#[test]
	fn test_wakeup_task_blocked() {
		let task = Rc::new(RefCell::new(Task {
			id: TaskId(1),
			status: TaskStatus::Blocked,
			prio: Priority(1 as u8),
			last_stack_pointer: x86::bits64::paging::VAddr(10),
			user_stack_pointer: x86::bits64::paging::VAddr(10),
			last_fpu_state: arch::processor::FPUState::new(),
			core_id: 0,
			stacks: TaskStacks::Common(CommonStack {
				virt_addr: x86::bits64::paging::VAddr::from_u64(2000),
				phys_addr: x86::bits64::paging::PAddr(2000),
				total_size: 4000,
			}),
			tls: None,
			#[cfg(feature = "newlib")]
			lwip_errno: 0,
		}));

		BlockedTaskQueue::wakeup_task(task.clone());

		let borrowed = task.borrow();
		assert_eq!(borrowed.status, TaskStatus::Ready);
	}

	#[test]
	#[should_panic(expected = "Try to wake up task")]
	fn test_wakeup_task_wrong_core() {
		let task = Rc::new(RefCell::new(Task {
			id: TaskId(1),
			status: TaskStatus::Blocked,
			prio: Priority(1 as u8),
			last_stack_pointer: x86::bits64::paging::VAddr(10),
			user_stack_pointer: x86::bits64::paging::VAddr(10),
			last_fpu_state: arch::processor::FPUState::new(),
			core_id: 2,
			stacks: TaskStacks::Common(CommonStack {
				virt_addr: x86::bits64::paging::VAddr::from_u64(2000),
				phys_addr: x86::bits64::paging::PAddr(2000),
				total_size: 4000,
			}),
			tls: None,
			#[cfg(feature = "newlib")]
			lwip_errno: 0,
		}));

		BlockedTaskQueue::wakeup_task(task.clone());
	}

	#[test]
	#[should_panic(expected = "Trying to wake up task")]
	fn test_wakeup_task_not_blocked() {
		let task = Rc::new(RefCell::new(Task {
			id: TaskId(1),
			status: TaskStatus::Ready,
			prio: Priority(1 as u8),
			last_stack_pointer: x86::bits64::paging::VAddr(10),
			user_stack_pointer: x86::bits64::paging::VAddr(10),
			last_fpu_state: arch::processor::FPUState::new(),
			core_id: 0,
			stacks: TaskStacks::Common(CommonStack {
				virt_addr: x86::bits64::paging::VAddr::from_u64(2000),
				phys_addr: x86::bits64::paging::PAddr(2000),
				total_size: 4000,
			}),
			tls: None,
			#[cfg(feature = "newlib")]
			lwip_errno: 0,
		}));

		BlockedTaskQueue::wakeup_task(task.clone());
	}

	#[test]
	fn test_custom_wakeup_with_network_wakeup_time() {
		let mut queue = BlockedTaskQueue {
			list: LinkedList::new(),
			network_wakeup_time: Some(50),
		};

		let task1 = Rc::new(RefCell::new(Task {
			id: TaskId(1),
			status: TaskStatus::Blocked,
			prio: Priority(1 as u8),
			last_stack_pointer: x86::bits64::paging::VAddr(10),
			user_stack_pointer: x86::bits64::paging::VAddr(10),
			last_fpu_state: arch::processor::FPUState::new(),
			core_id: 0,
			stacks: TaskStacks::Common(CommonStack {
				virt_addr: x86::bits64::paging::VAddr::from_u64(2000),
				phys_addr: x86::bits64::paging::PAddr(2000),
				total_size: 4000,
			}),
			tls: None,
			#[cfg(feature = "newlib")]
			lwip_errno: 0,
		}));

		let task2 = Rc::new(RefCell::new(Task {
			id: TaskId(2),
			status: TaskStatus::Blocked,
			prio: Priority(1 as u8),
			last_stack_pointer: x86::bits64::paging::VAddr(10),
			user_stack_pointer: x86::bits64::paging::VAddr(10),
			last_fpu_state: arch::processor::FPUState::new(),
			core_id: 0,
			stacks: TaskStacks::Common(CommonStack {
				virt_addr: x86::bits64::paging::VAddr::from_u64(2000),
				phys_addr: x86::bits64::paging::PAddr(2000),
				total_size: 4000,
			}),
			tls: None,
			#[cfg(feature = "newlib")]
			lwip_errno: 0,
		}));

		queue
			.list
			.push_back(BlockedTask::new(task1.clone(), Some(100)));
		queue
			.list
			.push_back(BlockedTask::new(task2.clone(), Some(6)));

		queue.custom_wakeup(TaskHandle::new(
			TaskId::from(1),
			Priority::from(1 as u8),
			#[cfg(feature = "smp")]
			0,
		));

		// Assert that task1 is woken up and removed from the list
		assert_eq!(task1.borrow().status, TaskStatus::Ready);
		assert!(queue
			.list
			.iter()
			.all(|node| node.task.borrow().id != TaskId(1)));
	}
	#[test]
	fn test_custom_wakeup_without_network_wakeup_time() {
		let mut queue = BlockedTaskQueue {
			list: LinkedList::new(),
			network_wakeup_time: None,
		};

		let task1 = Rc::new(RefCell::new(Task {
			id: TaskId(1),
			status: TaskStatus::Blocked,
			prio: Priority(1 as u8),
			last_stack_pointer: x86::bits64::paging::VAddr(10),
			user_stack_pointer: x86::bits64::paging::VAddr(10),
			last_fpu_state: arch::processor::FPUState::new(),
			core_id: 0,
			stacks: TaskStacks::Common(CommonStack {
				virt_addr: x86::bits64::paging::VAddr::from_u64(2000),
				phys_addr: x86::bits64::paging::PAddr(2000),
				total_size: 4000,
			}),
			tls: None,
			#[cfg(feature = "newlib")]
			lwip_errno: 0,
		}));

		let task2 = Rc::new(RefCell::new(Task {
			id: TaskId(2),
			status: TaskStatus::Blocked,
			prio: Priority(1 as u8),
			last_stack_pointer: x86::bits64::paging::VAddr(10),
			user_stack_pointer: x86::bits64::paging::VAddr(10),
			last_fpu_state: arch::processor::FPUState::new(),
			core_id: 0,
			stacks: TaskStacks::Common(CommonStack {
				virt_addr: x86::bits64::paging::VAddr::from_u64(2000),
				phys_addr: x86::bits64::paging::PAddr(2000),
				total_size: 4000,
			}),
			tls: None,
			#[cfg(feature = "newlib")]
			lwip_errno: 0,
		}));

		queue
			.list
			.push_back(BlockedTask::new(task1.clone(), Some(100)));
		queue
			.list
			.push_back(BlockedTask::new(task2.clone(), Some(5)));

		queue.custom_wakeup(TaskHandle::new(
			TaskId::from(2),
			Priority::from(1 as u8),
			#[cfg(feature = "smp")]
			0,
		));

		// Assert that task2 is woken up and removed from the list
		assert_eq!(task2.borrow().status, TaskStatus::Ready);
		assert!(queue
			.list
			.iter()
			.all(|node| node.task.borrow().id != TaskId(2)));
	}

	#[test]
	fn test_add_network_timer_with_wakeup_time() {
		let mut queue = BlockedTaskQueue {
			list: LinkedList::new(),
			network_wakeup_time: None,
		};

		let task = Rc::new(RefCell::new(Task {
			id: TaskId(2),
			status: TaskStatus::Blocked,
			prio: Priority(1 as u8),
			last_stack_pointer: x86::bits64::paging::VAddr(10),
			user_stack_pointer: x86::bits64::paging::VAddr(10),
			last_fpu_state: arch::processor::FPUState::new(),
			core_id: 0,
			stacks: TaskStacks::Common(CommonStack {
				virt_addr: x86::bits64::paging::VAddr::from_u64(2000),
				phys_addr: x86::bits64::paging::PAddr(2000),
				total_size: 4000,
			}),
			tls: None,
			#[cfg(feature = "newlib")]
			lwip_errno: 0,
		}));

		let wakeup_time = Some(100);

		queue.add_network_timer(wakeup_time);

		// Assert that the network_wakeup_time is set correctly
		assert_eq!(queue.network_wakeup_time, wakeup_time);
	}

	#[test]
	fn test_add_network_timer_without_wakeup_time() {
		let mut queue = BlockedTaskQueue {
			list: LinkedList::new(),
			network_wakeup_time: Some(50),
		};

		let task = Rc::new(RefCell::new(Task {
			id: TaskId(2),
			status: TaskStatus::Blocked,
			prio: Priority(1 as u8),
			last_stack_pointer: x86::bits64::paging::VAddr(10),
			user_stack_pointer: x86::bits64::paging::VAddr(10),
			last_fpu_state: arch::processor::FPUState::new(),
			core_id: 0,
			stacks: TaskStacks::Common(CommonStack {
				virt_addr: x86::bits64::paging::VAddr::from_u64(2000),
				phys_addr: x86::bits64::paging::PAddr(2000),
				total_size: 4000,
			}),
			tls: None,
			#[cfg(feature = "newlib")]
			lwip_errno: 0,
		}));

		queue.list.push_front(BlockedTask::new(task, Some(50)));

		queue.add_network_timer(None);

		// Assert that the network_wakeup_time is set correctly
		assert_eq!(queue.network_wakeup_time, None);
	}

	#[test]
	fn test_add_with_wakeup_time() {
		let mut queue = BlockedTaskQueue {
			list: LinkedList::new(),
			network_wakeup_time: None,
		};

		let task = Rc::new(RefCell::new(Task {
			id: TaskId(2),
			status: TaskStatus::Running,
			prio: Priority(1 as u8),
			last_stack_pointer: x86::bits64::paging::VAddr(10),
			user_stack_pointer: x86::bits64::paging::VAddr(10),
			last_fpu_state: arch::processor::FPUState::new(),
			core_id: 0,
			stacks: TaskStacks::Common(CommonStack {
				virt_addr: x86::bits64::paging::VAddr::from_u64(2000),
				phys_addr: x86::bits64::paging::PAddr(2000),
				total_size: 4000,
			}),
			tls: None,
			#[cfg(feature = "newlib")]
			lwip_errno: 0,
		}));

		let wakeup_time = Some(100);

		queue.add(task.clone(), wakeup_time);

		// Assert that the task status is set to Blocked
		assert_eq!(task.borrow().status, TaskStatus::Blocked);
	}

	#[test]
	fn test_add_without_wakeup_time() {
		let mut queue = BlockedTaskQueue {
			list: LinkedList::new(),
			network_wakeup_time: Some(50),
		};

		let task = Rc::new(RefCell::new(Task {
			id: TaskId(2),
			status: TaskStatus::Running,
			prio: Priority(1 as u8),
			last_stack_pointer: x86::bits64::paging::VAddr(10),
			user_stack_pointer: x86::bits64::paging::VAddr(10),
			last_fpu_state: arch::processor::FPUState::new(),
			core_id: 0,
			stacks: TaskStacks::Common(CommonStack {
				virt_addr: x86::bits64::paging::VAddr::from_u64(2000),
				phys_addr: x86::bits64::paging::PAddr(2000),
				total_size: 4000,
			}),
			tls: None,
			#[cfg(feature = "newlib")]
			lwip_errno: 0,
		}));

		queue.add(task.clone(), None);

		// Assert that the task status is set to Blocked
		assert_eq!(task.borrow().status, TaskStatus::Blocked);
	}
}
