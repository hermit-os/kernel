#![allow(clippy::type_complexity)]

#[cfg(not(feature = "common-os"))]
pub(crate) mod tls;

#[cfg(feature = "common-os")]
use alloc::collections::BTreeMap;
use alloc::collections::{LinkedList, VecDeque};
use alloc::rc::Rc;
use alloc::sync::Arc;
#[cfg(feature = "common-os")]
use alloc::vec::Vec;
use core::cell::RefCell;
use core::num::NonZeroU64;
use core::{cmp, fmt};

use ahash::RandomState;
use crossbeam_utils::CachePadded;
use hashbrown::HashMap;
#[cfg(not(feature = "common-os"))]
use hermit_sync::OnceCell;
use hermit_sync::RwSpinLock;
#[cfg(not(target_arch = "x86_64"))]
use memory_addresses::VirtAddr;
#[cfg(target_arch = "x86_64")]
use x86_64::VirtAddr;

#[cfg(not(feature = "common-os"))]
use self::tls::Tls;
use super::timer_interrupts::{Source, create_timer_abs};
#[cfg(feature = "common-os")]
use crate::arch;
use crate::arch::kernel::core_local::*;
use crate::arch::kernel::processor::{self, FPUState};
use crate::arch::kernel::scheduler::TaskStacks;
#[cfg(not(feature = "common-os"))]
use crate::fd::stdio;
use crate::fd::{Fd, RawFd};
#[cfg(feature = "common-os")]
use crate::mm::vma::VirtualMemoryArea;
use crate::scheduler::CoreId;

/// A reference-counted handle to a process's root page table.
///
/// Threads of the same process share the same `Arc<RootPageTable>`. When
/// the last owning task is dropped, the physical page-table hierarchy and
/// the user-space mappings are released.
#[cfg(feature = "common-os")]
pub struct RootPageTable {
	pml4_phys: usize,
	/// `false` for the boot page table, which must never be released.
	owned: bool,
}

#[cfg(feature = "common-os")]
impl RootPageTable {
	/// Wraps a freshly allocated PML4 that this process owns.
	pub fn new(pml4_phys: usize) -> Self {
		Self {
			pml4_phys,
			owned: true,
		}
	}

	/// Wraps the boot page table, shared by all idle tasks. Dropping this
	/// instance is a no-op.
	pub fn new_boot(pml4_phys: usize) -> Self {
		Self {
			pml4_phys,
			owned: false,
		}
	}

	pub fn as_usize(&self) -> usize {
		self.pml4_phys
	}
}

#[cfg(feature = "common-os")]
impl Drop for RootPageTable {
	fn drop(&mut self) {
		if self.owned {
			arch::mm::drop_user_space(self.pml4_phys);
		}
	}
}

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

/// Unique identifier for a task (i.e. `tid`).
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Copy)]
pub struct TaskId(i32);

impl TaskId {
	pub const fn from(x: i32) -> Self {
		TaskId(x)
	}
}

impl From<TaskId> for i32 {
	fn from(tid: TaskId) -> Self {
		tid.0
	}
}

impl fmt::Display for TaskId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}", self.0)
	}
}

/// Process ID — shared between every thread of the same process.
///
/// For the main thread of a process it carries the same numeric value
/// as the thread's [`TaskId`]; additional threads inherit it from the
/// spawning thread. A [`TaskId`] converts into a `ProcessId` via
/// [`From`] / [`Into`] (used by `Task::new` to seed `pid = tid` for a
/// newly created main thread).
#[cfg(feature = "common-os")]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Copy)]
pub struct ProcessId(i32);

#[cfg(feature = "common-os")]
impl ProcessId {
	pub const fn from(x: i32) -> Self {
		ProcessId(x)
	}
}

#[cfg(feature = "common-os")]
impl From<ProcessId> for i32 {
	fn from(pid: ProcessId) -> Self {
		pid.0
	}
}

#[cfg(feature = "common-os")]
impl From<TaskId> for ProcessId {
	fn from(tid: TaskId) -> Self {
		ProcessId(tid.0)
	}
}

#[cfg(feature = "common-os")]
impl fmt::Display for ProcessId {
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
			queues: [const { None }; NO_PRIORITIES],
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
		let queue = self.queues[queue_index].as_mut()?;

		let task = queue.pop_front();

		if queue.is_empty() {
			*self.prio_bitmap &= !(1 << queue_index as u64);
		}

		task
	}

	/// Pop the task handle with the highest priority from the queue
	pub fn pop(&mut self) -> Option<TaskHandle> {
		let i = msb(self.prio_bitmap.into_inner())?;

		self.pop_from_queue(i as usize)
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
		if queue.len() < task_index {
			return None;
		}

		// Calling remove is unstable: https://github.com/rust-lang/rust/issues/69210
		let mut split_list = queue.split_off(task_index);
		let element = split_list.pop_front();
		queue.append(&mut split_list);
		if queue.is_empty() {
			self.prio_bitmap &= !(1 << queue_index as u64);
		}
		element
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
		let i = msb(self.prio_bitmap)?;

		self.pop_from_queue(i as usize)
	}

	/// Pop the next task, which has a higher or the same priority as `prio`
	pub fn pop_with_prio(&mut self, prio: Priority) -> Option<Rc<RefCell<Task>>> {
		let i = msb(self.prio_bitmap)?;

		if i < u32::from(prio.into()) {
			return None;
		}

		self.pop_from_queue(i as usize)
	}

	/// Returns the highest priority of all available task
	#[cfg(all(any(target_arch = "x86_64", target_arch = "riscv64"), feature = "smp"))]
	pub fn get_highest_priority(&self) -> Priority {
		let Some(i) = msb(self.prio_bitmap) else {
			return IDLE_PRIO;
		};

		Priority::from(i.try_into().unwrap())
	}

	/// Change priority of specific task
	pub fn set_priority(&mut self, handle: TaskHandle, prio: Priority) -> Result<(), ()> {
		let old_priority = handle.get_priority().into() as usize;
		let index = self.queues[old_priority]
			.iter()
			.position(|current_task| current_task.borrow().id == handle.id)
			.ok_or(())?;

		let task = self.remove_from_queue(index, old_priority).ok_or(())?;
		task.borrow_mut().prio = prio;
		self.push(task);
		Ok(())
	}
}

/// Per-process TLS template used to initialize newly spawned threads.
///
/// `size` is the byte size of the TLS data block (the offset at which the
/// TCB / thread pointer lives, also the value that the parent's main thread
/// uses for its own block). `init` is a snapshot of the freshly-loaded TLS
/// image taken right after the user binary's `PT_TLS` segment was copied
/// into place by `load_application`. Each new thread starts with a verbatim
/// copy of `init`, so it sees pristine `#[thread_local]` defaults rather
/// than mutated state from another thread.
#[cfg(feature = "common-os")]
pub(crate) struct TlsTemplate {
	pub size: usize,
	pub init: Vec<u8>,
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
	/// Process ID — equal to `id` for the main thread of a process, and
	/// inherited from the spawning thread for every additional thread of
	/// the same process.
	#[cfg(feature = "common-os")]
	pub pid: ProcessId,
	/// Status of a task, e.g. if the task is ready or blocked
	pub status: TaskStatus,
	/// Task priority,
	pub prio: Priority,
	/// Last stack pointer before a context switch to another task
	pub last_stack_pointer: VirtAddr,
	/// Last stack pointer on the user stack before jumping to kernel space
	pub user_stack_pointer: VirtAddr,
	/// Last FPU state before a context switch to another task using the FPU
	pub last_fpu_state: FPUState,
	/// ID of the core this task is running on
	pub core_id: CoreId,
	/// Stack of the task
	pub stacks: TaskStacks,
	/// Mapping between file descriptor and the referenced IO interface
	pub object_map: Arc<RwSpinLock<HashMap<RawFd, Arc<async_lock::RwLock<Fd>>, RandomState>>>,
	/// Task Thread-Local-Storage (TLS)
	#[cfg(not(feature = "common-os"))]
	pub tls: Option<Tls>,
	// Physical address of the 1st level page table, shared between all
	// threads of the same process via `Arc`. The address space is freed when
	// the last thread referencing this `RootPageTable` is dropped.
	#[cfg(feature = "common-os")]
	pub root_page_table: Arc<RootPageTable>,
	/// Per-process TLS template used to allocate fresh TLS regions for new
	/// threads. `None` for kernel-only tasks; set by `load_application`
	/// when the user binary has a `PT_TLS` segment.
	#[cfg(feature = "common-os")]
	pub tls_template: Option<Arc<TlsTemplate>>,
	#[cfg(feature = "common-os")]
	pub vmas: Arc<RwSpinLock<BTreeMap<VirtAddr, VirtualMemoryArea>>>,
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
		object_map: Arc<RwSpinLock<HashMap<RawFd, Arc<async_lock::RwLock<Fd>>, RandomState>>>,
	) -> Task {
		debug!("Creating new task {tid} on core {core_id}");

		Task {
			id: tid,
			#[cfg(feature = "common-os")]
			pid: tid.into(),
			status: task_status,
			prio: task_prio,
			last_stack_pointer: VirtAddr::zero(),
			user_stack_pointer: VirtAddr::zero(),
			last_fpu_state: FPUState::new(),
			core_id,
			stacks,
			object_map,
			#[cfg(not(feature = "common-os"))]
			tls: None,
			#[cfg(feature = "common-os")]
			root_page_table: Arc::new(RootPageTable::new(arch::mm::create_new_root_page_table())),
			#[cfg(feature = "common-os")]
			tls_template: None,
			#[cfg(feature = "common-os")]
			vmas: Arc::new(RwSpinLock::new(BTreeMap::new())),
		}
	}

	pub fn new_idle(tid: TaskId, core_id: CoreId) -> Task {
		debug!("Creating idle task {tid}");

		/// In the unikernel case all cores share the same mapping between
		/// file descriptor and the referenced object. Under `common-os`,
		/// each process gets its own `object_map` when the application is
		/// loaded, so the idle task only needs an empty placeholder.
		#[cfg(not(feature = "common-os"))]
		static OBJECT_MAP: OnceCell<
			Arc<RwSpinLock<HashMap<RawFd, Arc<async_lock::RwLock<Fd>>, RandomState>>>,
		> = OnceCell::new();

		#[cfg(not(feature = "common-os"))]
		if core_id == 0 {
			OBJECT_MAP
				.set(Arc::new(RwSpinLock::new(HashMap::<
					RawFd,
					Arc<async_lock::RwLock<Fd>>,
					RandomState,
				>::with_hasher(
					RandomState::with_seeds(0, 0, 0, 0),
				))))
				// This function is called once per core and thus only once on core 0.
				// Thus, this is the only place where we set OBJECT_MAP.
				.unwrap_or_else(|_| unreachable!());
			let objmap = OBJECT_MAP.get().unwrap().clone();
			stdio::setup(&mut objmap.write());
		}

		#[cfg(not(feature = "common-os"))]
		let tls = if cfg!(feature = "instrument-mcount") {
			Tls::from_env().inspect(Tls::set_thread_ptr)
		} else {
			None
		};

		Task {
			id: tid,
			#[cfg(feature = "common-os")]
			pid: tid.into(),
			status: TaskStatus::Idle,
			prio: IDLE_PRIO,
			last_stack_pointer: VirtAddr::zero(),
			user_stack_pointer: VirtAddr::zero(),
			last_fpu_state: FPUState::new(),
			core_id,
			stacks: TaskStacks::from_boot_stacks(),
			#[cfg(not(feature = "common-os"))]
			object_map: OBJECT_MAP.get().unwrap().clone(),
			#[cfg(feature = "common-os")]
			object_map: Arc::new(RwSpinLock::new(HashMap::<
				RawFd,
				Arc<async_lock::RwLock<Fd>>,
				RandomState,
			>::with_hasher(RandomState::with_seeds(
				0, 0, 0, 0,
			)))),
			#[cfg(not(feature = "common-os"))]
			tls,
			#[cfg(feature = "common-os")]
			root_page_table: Arc::new(RootPageTable::new_boot(
				*crate::scheduler::BOOT_ROOT_PAGE_TABLE.get().unwrap(),
			)),
			#[cfg(feature = "common-os")]
			tls_template: None,
			#[cfg(feature = "common-os")]
			vmas: Arc::new(RwSpinLock::new(BTreeMap::new())),
		}
	}

	/// Create a new user-space thread that shares its parent's address space.
	///
	/// The `root_page_table` `Arc` is cloned from the parent, so all threads
	/// of the same process drop together when the last one exits.
	/// `parent_pid` is the spawning thread's `pid`; the new thread joins
	/// the same process and reports the same `pid` value.
	#[cfg(feature = "common-os")]
	#[allow(clippy::too_many_arguments)]
	pub fn new_thread(
		tid: TaskId,
		parent_pid: ProcessId,
		core_id: CoreId,
		task_status: TaskStatus,
		task_prio: Priority,
		stacks: TaskStacks,
		object_map: Arc<RwSpinLock<HashMap<RawFd, Arc<async_lock::RwLock<Fd>>, RandomState>>>,
		root_page_table: Arc<RootPageTable>,
		tls_template: Option<Arc<TlsTemplate>>,
		vmas: Arc<RwSpinLock<BTreeMap<VirtAddr, VirtualMemoryArea>>>,
	) -> Task {
		debug!("Creating user thread {tid} (pid {parent_pid}) on core {core_id}");
		Task {
			id: tid,
			pid: parent_pid,
			status: task_status,
			prio: task_prio,
			last_stack_pointer: VirtAddr::zero(),
			user_stack_pointer: VirtAddr::zero(),
			last_fpu_state: FPUState::new(),
			core_id,
			stacks,
			object_map,
			root_page_table,
			tls_template,
			vmas,
		}
	}
}

impl Drop for Task {
	fn drop(&mut self) {
		//debug!("Drop task {}", self.id);
	}
}

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
}

impl BlockedTaskQueue {
	pub const fn new() -> Self {
		Self {
			list: LinkedList::new(),
		}
	}

	fn mark_ready(task: &RefCell<Task>) {
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

			while let Some(node) = cursor.current() {
				let node_wakeup_time = node.wakeup_time;
				if node_wakeup_time.is_none() || wt < node_wakeup_time.unwrap() {
					cursor.insert_before(new_node);

					create_timer_abs(Source::Scheduler, wt);
					return;
				}

				cursor.move_next();
			}

			create_timer_abs(Source::Scheduler, wt);
		}

		self.list.push_back(new_node);
	}

	/// Manually wake up a blocked task.
	pub fn custom_wakeup(&mut self, task: TaskHandle) -> Rc<RefCell<Task>> {
		let mut first_task = true;
		let mut cursor = self.list.cursor_front_mut();

		// Loop through all blocked tasks to find it.
		while let Some(node) = cursor.current() {
			if node.task.borrow().id == task.get_id() {
				// Remove it from the list of blocked tasks.
				let task_ref = node.task.clone();
				cursor.remove_current();

				// If this is the first task, adjust the One-Shot Timer to fire at the
				// next task's wakeup time (if any).
				if first_task
					&& let Some(wakeup) = cursor
						.current()
						.map_or_else(|| None, |node| node.wakeup_time)
				{
					create_timer_abs(Source::Scheduler, wakeup);
				}

				// Wake it up.
				Self::mark_ready(&task_ref);

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
	pub fn handle_waiting_tasks(&mut self, ready_queue: &mut PriorityTaskQueue) {
		// Get the current time.
		let time = processor::get_timer_ticks();

		// Get the wakeup time of this task and check if we have reached the first task
		// that hasn't elapsed yet or waits indefinitely.
		// This iterator has to be consumed to actually remove the elements.
		let newly_ready_tasks = self.list.extract_if(|blocked_task| {
			blocked_task
				.wakeup_time
				.is_some_and(|wakeup_time| wakeup_time < time)
		});

		for task in newly_ready_tasks {
			Self::mark_ready(&task.task);
			ready_queue.push(task.task);
		}

		let new_task_wakeup_time = self.list.front().and_then(|task| task.wakeup_time);

		if let Some(wakeup) = new_task_wakeup_time {
			create_timer_abs(Source::Scheduler, wakeup);
		}
	}
}
