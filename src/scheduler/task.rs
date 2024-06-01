#![allow(clippy::type_complexity)]

#[cfg(not(feature = "common-os"))]
use alloc::boxed::Box;
use alloc::collections::{LinkedList, VecDeque};
use alloc::rc::Rc;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::num::NonZeroU64;
#[cfg(any(feature = "tcp", feature = "udp"))]
use core::ops::DerefMut;
use core::{cmp, fmt};

use ahash::RandomState;
use crossbeam_utils::CachePadded;
use hashbrown::HashMap;
use hermit_sync::OnceCell;

use crate::arch::core_local::*;
use crate::arch::mm::VirtAddr;
use crate::arch::scheduler::TaskStacks;
#[cfg(not(feature = "common-os"))]
use crate::arch::scheduler::TaskTLS;
use crate::executor::poll_on;
use crate::fd::stdio::*;
use crate::fd::{
	FileDescriptor, IoError, ObjectInterface, STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO,
};
use crate::scheduler::CoreId;
use crate::{arch, env};

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
pub(crate) enum TaskStatus {
	Invalid,
	Ready,
	Running,
	Blocked,
	Finished,
	Idle,
}

/// Unique identifier for a task (i.e. `pid`).
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Copy)]
pub struct TaskId(i32);

impl TaskId {
	pub const fn into(self) -> i32 {
		self.0
	}

	pub const fn from(x: i32) -> Self {
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
pub(crate) struct TaskHandle {
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
	fn cmp(&self, other: &Self) -> cmp::Ordering {
		self.id.cmp(&other.id)
	}
}

impl PartialOrd for TaskHandle {
	fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
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
pub(crate) struct TaskHandlePriorityQueue {
	queues: [Option<VecDeque<TaskHandle>>; NO_PRIORITIES],
	prio_bitmap: CachePadded<u64>,
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
			prio_bitmap: CachePadded::new(0),
		}
	}

	/// Checks if the queue is empty.
	pub fn is_empty(&self) -> bool {
		self.prio_bitmap.into_inner() == 0
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

		*self.prio_bitmap |= (1 << i) as u64;
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
				*self.prio_bitmap &= !(1 << queue_index as u64);
			}

			task
		} else {
			None
		}
	}

	/// Pop the task handle with the highest priority from the queue
	pub fn pop(&mut self) -> Option<TaskHandle> {
		if let Some(i) = msb(self.prio_bitmap.into_inner()) {
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
				*self.prio_bitmap &= !(1 << queue_index as u64);
			}
		}

		success
	}
}

/// Realize a priority queue for tasks
pub(crate) struct PriorityTaskQueue {
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

	/// Returns reference to prio_bitmap
	#[allow(dead_code)]
	#[inline]
	pub fn get_priority_bitmap(&self) -> &u64 {
		&self.prio_bitmap
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
			if i >= prio.into().into() {
				return self.pop_from_queue(i as usize);
			}
		}

		None
	}

	/// Returns the highest priority of all available task
	#[cfg(all(any(target_arch = "x86_64", target_arch = "riscv64"), feature = "smp"))]
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
pub(crate) struct Task {
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
	#[cfg(any(target_arch = "x86_64", target_arch = "riscv64"))]
	pub last_fpu_state: arch::processor::FPUState,
	/// ID of the core this task is running on
	pub core_id: CoreId,
	/// Stack of the task
	pub stacks: TaskStacks,
	/// Mapping between file descriptor and the referenced IO interface
	pub object_map:
		Arc<async_lock::RwLock<HashMap<FileDescriptor, Arc<dyn ObjectInterface>, RandomState>>>,
	/// Task Thread-Local-Storage (TLS)
	#[cfg(not(feature = "common-os"))]
	pub tls: Option<Box<TaskTLS>>,
	// Physical address of the 1st level page table
	#[cfg(all(target_arch = "x86_64", feature = "common-os"))]
	pub root_page_table: usize,
}

pub(crate) trait TaskFrame {
	/// Create the initial stack frame for a new task
	fn create_stack_frame(&mut self, func: unsafe extern "C" fn(usize), arg: usize);
}

impl Task {
	pub fn new(
		tid: TaskId,
		core_id: CoreId,
		task_status: TaskStatus,
		task_prio: Priority,
		stacks: TaskStacks,
		object_map: Arc<
			async_lock::RwLock<HashMap<FileDescriptor, Arc<dyn ObjectInterface>, RandomState>>,
		>,
	) -> Task {
		debug!("Creating new task {} on core {}", tid, core_id);

		Task {
			id: tid,
			status: task_status,
			prio: task_prio,
			last_stack_pointer: VirtAddr(0u64),
			user_stack_pointer: VirtAddr(0u64),
			#[cfg(any(target_arch = "x86_64", target_arch = "riscv64"))]
			last_fpu_state: arch::processor::FPUState::new(),
			core_id,
			stacks,
			object_map,
			#[cfg(not(feature = "common-os"))]
			tls: None,
			#[cfg(all(target_arch = "x86_64", feature = "common-os"))]
			root_page_table: arch::create_new_root_page_table(),
		}
	}

	pub fn new_idle(tid: TaskId, core_id: CoreId) -> Task {
		debug!("Creating idle task {}", tid);

		/// All cores use the same mapping between file descriptor and the referenced object
		static OBJECT_MAP: OnceCell<
			Arc<async_lock::RwLock<HashMap<FileDescriptor, Arc<dyn ObjectInterface>, RandomState>>>,
		> = OnceCell::new();

		if core_id == 0 {
			OBJECT_MAP
				.set(Arc::new(async_lock::RwLock::new(HashMap::<
					FileDescriptor,
					Arc<dyn ObjectInterface>,
					RandomState,
				>::with_hasher(
					RandomState::with_seeds(0, 0, 0, 0),
				))))
				.unwrap();
			let objmap = OBJECT_MAP.get().unwrap().clone();
			let _ = poll_on(
				async {
					let mut guard = objmap.write().await;
					if env::is_uhyve() {
						guard
							.try_insert(STDIN_FILENO, Arc::new(UhyveStdin::new()))
							.map_err(|_| IoError::EIO)?;
						guard
							.try_insert(STDOUT_FILENO, Arc::new(UhyveStdout::new()))
							.map_err(|_| IoError::EIO)?;
						guard
							.try_insert(STDERR_FILENO, Arc::new(UhyveStderr::new()))
							.map_err(|_| IoError::EIO)?;
					} else {
						guard
							.try_insert(STDIN_FILENO, Arc::new(GenericStdin::new()))
							.map_err(|_| IoError::EIO)?;
						guard
							.try_insert(STDOUT_FILENO, Arc::new(GenericStdout::new()))
							.map_err(|_| IoError::EIO)?;
						guard
							.try_insert(STDERR_FILENO, Arc::new(GenericStderr::new()))
							.map_err(|_| IoError::EIO)?;
					}

					Ok(())
				},
				None,
			);
		}

		Task {
			id: tid,
			status: TaskStatus::Idle,
			prio: IDLE_PRIO,
			last_stack_pointer: VirtAddr(0u64),
			user_stack_pointer: VirtAddr(0u64),
			#[cfg(any(target_arch = "x86_64", target_arch = "riscv64"))]
			last_fpu_state: arch::processor::FPUState::new(),
			core_id,
			stacks: TaskStacks::from_boot_stacks(),
			object_map: OBJECT_MAP.get().unwrap().clone(),
			#[cfg(not(feature = "common-os"))]
			tls: None,
			#[cfg(all(target_arch = "x86_64", feature = "common-os"))]
			root_page_table: *crate::scheduler::BOOT_ROOT_PAGE_TABLE.get().unwrap(),
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

pub(crate) struct BlockedTaskQueue {
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
	pub fn custom_wakeup(&mut self, task: TaskHandle) -> Rc<RefCell<Task>> {
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
				// Remove it from the list of blocked tasks.
				let task_ref = node.task.clone();
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

				// Wake it up.
				Self::wakeup_task(task_ref.clone());

				return task_ref;
			}

			first_task = false;
			cursor.move_next();
		}

		unreachable!();
	}

	/// Wakes up all tasks whose wakeup time has elapsed.
	///
	/// Should be called by the One-Shot Timer interrupt handler when the wakeup time for
	/// at least one task has elapsed.
	pub fn handle_waiting_tasks(&mut self) -> Vec<Rc<RefCell<Task>>> {
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

		let mut tasks = vec![];

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
			tasks.push(node.task.clone());
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

		for task in tasks.iter().cloned() {
			Self::wakeup_task(task);
		}

		tasks
	}
}
