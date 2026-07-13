#![allow(clippy::type_complexity)]

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::rc::Rc;
use alloc::sync::Arc;
#[cfg(feature = "smp")]
use alloc::vec::Vec;
use core::cell::RefCell;
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use ahash::RandomState;
use crossbeam_utils::Backoff;
use hashbrown::{HashMap, hash_map};
use hermit_sync::*;
#[cfg(target_arch = "aarch64")]
use memory_addresses::VirtAddr;
#[cfg(all(target_arch = "riscv64", feature = "common-os"))]
use memory_addresses::VirtAddr;
#[cfg(target_arch = "riscv64")]
use riscv::register::sstatus;
use timer_interrupts::TimerList;
#[cfg(all(feature = "common-os", target_arch = "x86_64"))]
use x86_64::VirtAddr;

use crate::arch::kernel;
use crate::arch::kernel::core_local::*;
use crate::arch::kernel::scheduler::TaskStacks;
#[cfg(target_arch = "riscv64")]
use crate::arch::kernel::switch::switch_to_task;
#[cfg(target_arch = "x86_64")]
use crate::arch::kernel::switch::{switch_to_fpu_owner, switch_to_task};
use crate::arch::kernel::{get_processor_count, interrupts};
use crate::errno::Errno;
use crate::fd::{Fd, RawFd};
use crate::io;
#[cfg(feature = "common-os")]
use crate::mm::vma::VirtualMemoryArea;
use crate::scheduler::task::*;

pub mod task;
pub mod timer_interrupts;

static NO_TASKS: AtomicU32 = AtomicU32::new(0);
/// Map between Core ID and per-core scheduler
#[cfg(feature = "smp")]
static SCHEDULER_INPUTS: SpinMutex<Vec<&InterruptTicketMutex<SchedulerInput>>> =
	SpinMutex::new(Vec::new());
/// Map between Task ID and Queue of waiting tasks
static WAITING_TASKS: InterruptTicketMutex<BTreeMap<TaskId, VecDeque<TaskHandle>>> =
	InterruptTicketMutex::new(BTreeMap::new());
/// Map between Task ID and TaskHandle
static TASKS: InterruptTicketMutex<BTreeMap<TaskId, TaskHandle>> =
	InterruptTicketMutex::new(BTreeMap::new());
/// Count the number of spawned tasks
static SPAWN_COUNTER: AtomicU32 = AtomicU32::new(1);

/// Unique identifier for a core.
pub type CoreId = u32;

#[cfg(feature = "smp")]
pub(crate) struct SchedulerInput {
	/// Queue of new tasks
	new_tasks: VecDeque<NewTask>,
	/// Queue of task, which are wakeup by another core
	wakeup_tasks: VecDeque<TaskHandle>,
}

#[cfg(feature = "smp")]
impl SchedulerInput {
	pub fn new() -> Self {
		Self {
			new_tasks: VecDeque::new(),
			wakeup_tasks: VecDeque::new(),
		}
	}
}

#[cfg_attr(any(target_arch = "x86_64", target_arch = "aarch64"), repr(align(128)))]
#[cfg_attr(
	not(any(target_arch = "x86_64", target_arch = "aarch64")),
	repr(align(64))
)]
pub(crate) struct PerCoreScheduler {
	/// Core ID of this per-core scheduler
	#[cfg(feature = "smp")]
	core_id: CoreId,
	/// Task which is currently running
	current_task: Rc<RefCell<Task>>,
	/// Idle Task
	idle_task: Rc<RefCell<Task>>,
	/// Task that currently owns the FPU
	#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
	fpu_owner: Rc<RefCell<Task>>,
	/// Queue of tasks, which are ready
	ready_queue: PriorityTaskQueue,
	/// Queue of tasks, which are finished and can be released
	finished_tasks: VecDeque<Rc<RefCell<Task>>>,
	/// Queue of blocked tasks, sorted by wakeup time.
	blocked_tasks: BlockedTaskQueue,
	/// Queue of timer interrupts.
	pub timers: TimerList,
}

pub(crate) trait PerCoreSchedulerExt {
	/// Triggers the scheduler to reschedule the tasks.
	/// Interrupt flag will be cleared during the reschedule
	fn reschedule(self);

	/// Terminate the current task on the current core.
	fn exit(self, exit_code: i32) -> !;
}

impl PerCoreSchedulerExt for &mut PerCoreScheduler {
	#[cfg(target_arch = "x86_64")]
	fn reschedule(self) {
		without_interrupts(|| {
			let Some(last_stack_pointer) = self.scheduler() else {
				return;
			};

			let (new_stack_pointer, is_idle) = {
				let borrowed = self.current_task.borrow();
				(
					borrowed.last_stack_pointer,
					borrowed.status == TaskStatus::Idle,
				)
			};

			if is_idle || Rc::ptr_eq(&self.current_task, &self.fpu_owner) {
				unsafe {
					switch_to_fpu_owner(last_stack_pointer, new_stack_pointer.as_u64() as usize);
				}
			} else {
				unsafe {
					switch_to_task(last_stack_pointer, new_stack_pointer.as_u64() as usize);
				}
			}
		});
	}

	/// Trigger an interrupt to reschedule the system
	#[cfg(target_arch = "aarch64")]
	fn reschedule(self) {
		use aarch64_cpu::asm::barrier::{NSH, SY, dsb, isb};
		use arm_gic::IntId;
		use arm_gic::gicv3::{GicCpuInterface, SgiTarget, SgiTargetGroup};

		use crate::arch::kernel::interrupts::SGI_RESCHED;

		dsb(NSH);
		isb(SY);

		let reschedid = IntId::sgi(SGI_RESCHED.into());
		#[cfg(feature = "smp")]
		let core_id = self.core_id;
		#[cfg(not(feature = "smp"))]
		let core_id = 0;

		GicCpuInterface::send_sgi(
			reschedid,
			SgiTarget::List {
				affinity3: 0,
				affinity2: 0,
				affinity1: 0,
				target_list: 1 << core_id,
			},
			SgiTargetGroup::CurrentGroup1,
		)
		.unwrap();

		interrupts::enable();
	}

	#[cfg(target_arch = "riscv64")]
	fn reschedule(self) {
		without_interrupts(|| self.scheduler());
	}

	fn exit(self, exit_code: i32) -> ! {
		without_interrupts(|| {
			// Get the current task.
			let mut current_task_borrowed = self.current_task.borrow_mut();
			assert_ne!(
				current_task_borrowed.status,
				TaskStatus::Idle,
				"Trying to terminate the idle task"
			);

			// Finish the task and reschedule.
			debug!(
				"Finishing task {} with exit code {}",
				current_task_borrowed.id, exit_code
			);
			current_task_borrowed.status = TaskStatus::Finished;
			NO_TASKS.fetch_sub(1, Ordering::SeqCst);

			let current_id = current_task_borrowed.id;
			drop(current_task_borrowed);

			// wakeup tasks, which are waiting for task with the identifier id
			if let Some(mut queue) = WAITING_TASKS.lock().remove(&current_id) {
				while let Some(task) = queue.pop_front() {
					self.custom_wakeup(task);
				}
			}

			TASKS.lock().remove(&current_id);
		});

		self.reschedule();
		unreachable!()
	}
}

struct NewTask {
	tid: TaskId,
	func: unsafe extern "C" fn(usize),
	arg: usize,
	prio: Priority,
	core_id: CoreId,
	stacks: TaskStacks,
	object_map: Arc<RwSpinLock<HashMap<RawFd, Arc<async_lock::RwLock<Fd>>, RandomState>>>,
	/// When `Some`, the new task is a user-space thread that shares the
	/// given root page table with its parent process and inherits the
	/// parent's `pid`. When `None`, the task is a regular kernel-mode
	/// task with a fresh address space; its `pid` equals its `tid`.
	#[cfg(feature = "common-os")]
	thread_of: Option<(Arc<RootPageTable>, ProcessId)>,
	/// Per-process TLS template, cloned from the spawning thread. Used by
	/// `From<NewTask>` to propagate the template into the new task so that
	/// any threads it spawns in turn can allocate their own TLS regions.
	#[cfg(feature = "common-os")]
	tls_template: Option<Arc<TlsTemplate>>,
	/// Per-thread TLS thread-pointer (FS.Base on x86_64, TPIDR_EL0 on
	/// aarch64), already prepared by `spawn_thread` from the per-process
	/// `TlsTemplate`. Zero means "do not install a thread pointer".
	#[cfg(feature = "common-os")]
	tls_base: u64,
	#[cfg(feature = "common-os")]
	vmas: Arc<RwSpinLock<BTreeMap<VirtAddr, VirtualMemoryArea>>>,
}

impl From<NewTask> for Task {
	fn from(value: NewTask) -> Self {
		let NewTask {
			tid,
			func,
			arg,
			prio,
			core_id,
			stacks,
			object_map,
			#[cfg(feature = "common-os")]
			thread_of,
			#[cfg(feature = "common-os")]
			tls_template,
			#[cfg(feature = "common-os")]
			tls_base,
			#[cfg(feature = "common-os")]
			vmas,
		} = value;

		#[cfg(feature = "common-os")]
		if let Some((root_page_table, parent_pid)) = thread_of {
			let mut task = Self::new_thread(
				tid,
				parent_pid,
				core_id,
				TaskStatus::Ready,
				prio,
				stacks,
				object_map,
				root_page_table,
				tls_template,
				vmas,
			);
			task.create_user_stack_frame(func, arg, tls_base);
			return task;
		}

		let mut task = Self::new(tid, core_id, TaskStatus::Ready, prio, stacks, object_map);
		task.create_stack_frame(func, arg);
		task
	}
}

impl PerCoreScheduler {
	/// Spawn a new task.
	pub unsafe fn spawn(
		func: unsafe extern "C" fn(usize),
		arg: usize,
		prio: Priority,
		core_id: CoreId,
		stack_size: usize,
	) -> TaskId {
		// Create the new task.
		let tid = get_tid();
		let stacks = TaskStacks::new(stack_size);
		let new_task = NewTask {
			tid,
			func,
			arg,
			prio,
			core_id,
			stacks,
			object_map: core_scheduler().get_current_task_object_map(),
			#[cfg(feature = "common-os")]
			thread_of: None,
			#[cfg(feature = "common-os")]
			tls_template: None,
			#[cfg(feature = "common-os")]
			tls_base: 0,
			#[cfg(feature = "common-os")]
			vmas: Arc::new(RwSpinLock::new(BTreeMap::new())),
		};

		// Add it to the task lists.
		let wakeup = {
			#[cfg(feature = "smp")]
			let mut input_locked = get_scheduler_input(core_id).lock();
			WAITING_TASKS.lock().insert(tid, VecDeque::with_capacity(1));
			TASKS.lock().insert(
				tid,
				TaskHandle::new(
					tid,
					prio,
					#[cfg(feature = "smp")]
					core_id,
				),
			);
			NO_TASKS.fetch_add(1, Ordering::SeqCst);

			#[cfg(feature = "smp")]
			if core_id == core_scheduler().core_id {
				let task = Rc::new(RefCell::new(Task::from(new_task)));
				core_scheduler().ready_queue.push(task);
				false
			} else {
				input_locked.new_tasks.push_back(new_task);
				true
			}
			#[cfg(not(feature = "smp"))]
			if core_id == 0 {
				let task = Rc::new(RefCell::new(Task::from(new_task)));
				core_scheduler().ready_queue.push(task);
				false
			} else {
				panic!("Invalid core_id {core_id}!")
			}
		};

		debug!("Creating task {tid} with priority {prio} on core {core_id}");

		if wakeup {
			kernel::wakeup_core(core_id);
		}

		tid
	}

	/// Spawn a new user-space thread that shares the current task's
	/// address space (root page table).
	///
	/// `func` must be a valid ring-3 entry point mapped in the shared
	/// address space. The new thread receives its own kernel/interrupt
	/// stacks and a fresh user stack, all mapped into the shared PT via
	/// the regular `TaskStacks::new` path.
	#[cfg(feature = "common-os")]
	pub unsafe fn spawn_thread(
		func: unsafe extern "C" fn(usize),
		arg: usize,
		prio: Priority,
		core_id: CoreId,
		stack_size: usize,
	) -> TaskId {
		let tid = get_tid();
		// TaskStacks::new maps into the current address space — which *is*
		// the shared PT of the calling thread — so the new stacks are
		// immediately visible to every thread in this process.
		let stacks = TaskStacks::new(stack_size);

		let (root_page_table, object_map, tls_template, vmas, parent_pid) = {
			let current = core_scheduler().get_current_task();
			let borrowed = current.borrow();
			(
				borrowed.root_page_table.clone(),
				borrowed.object_map.clone(),
				borrowed.tls_template.clone(),
				borrowed.vmas.clone(),
				borrowed.pid,
			)
		};

		// Allocate a private TLS region for the new thread from the pristine
		// per-process TLS template captured at `load_application` time. Must
		// run in the parent's address space (still active here in the syscall
		// path) so that the new user-accessible pages get mapped into the
		// shared root page table.
		let tls_base = if let Some(ref template) = tls_template {
			crate::arch::mm::allocate_thread_tls(template)
		} else {
			0
		};

		let new_task = NewTask {
			tid,
			func,
			arg,
			prio,
			core_id,
			stacks,
			object_map,
			thread_of: Some((root_page_table, parent_pid)),
			tls_template,
			tls_base,
			vmas,
		};

		let wakeup = {
			#[cfg(feature = "smp")]
			let mut input_locked = get_scheduler_input(core_id).lock();
			WAITING_TASKS.lock().insert(tid, VecDeque::with_capacity(1));
			TASKS.lock().insert(
				tid,
				TaskHandle::new(
					tid,
					prio,
					#[cfg(feature = "smp")]
					core_id,
				),
			);
			NO_TASKS.fetch_add(1, Ordering::SeqCst);

			#[cfg(feature = "smp")]
			if core_id == core_scheduler().core_id {
				let task = Rc::new(RefCell::new(Task::from(new_task)));
				core_scheduler().ready_queue.push(task);
				false
			} else {
				input_locked.new_tasks.push_back(new_task);
				true
			}
			#[cfg(not(feature = "smp"))]
			if core_id == 0 {
				let task = Rc::new(RefCell::new(Task::from(new_task)));
				core_scheduler().ready_queue.push(task);
				false
			} else {
				panic!("Invalid core_id {core_id}!")
			}
		};

		debug!("Creating user thread {tid} with priority {prio} on core {core_id}");

		if wakeup {
			kernel::wakeup_core(core_id);
		}

		tid
	}

	#[cfg(feature = "newlib")]
	fn clone_impl(&self, func: extern "C" fn(usize), arg: usize) -> TaskId {
		static NEXT_CORE_ID: AtomicU32 = AtomicU32::new(1);

		// Get the Core ID of the next CPU.
		let core_id: CoreId = {
			// Increase the CPU number by 1.
			let id = NEXT_CORE_ID.fetch_add(1, Ordering::SeqCst);

			// Check for overflow.
			if id == get_processor_count() {
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
		let clone_task = NewTask {
			tid,
			func,
			arg,
			prio: current_task_borrowed.prio,
			core_id,
			stacks: TaskStacks::new(current_task_borrowed.stacks.get_user_stack_size()),
			object_map: current_task_borrowed.object_map.clone(),
			#[cfg(feature = "common-os")]
			thread_of: None,
			#[cfg(feature = "common-os")]
			tls_template: None,
			#[cfg(feature = "common-os")]
			tls_base: 0,
		};

		// Add it to the task lists.
		let wakeup = {
			#[cfg(feature = "smp")]
			let mut input_locked = get_scheduler_input(core_id).lock();
			WAITING_TASKS.lock().insert(tid, VecDeque::with_capacity(1));
			TASKS.lock().insert(
				tid,
				TaskHandle::new(
					tid,
					current_task_borrowed.prio,
					#[cfg(feature = "smp")]
					core_id,
				),
			);
			NO_TASKS.fetch_add(1, Ordering::SeqCst);
			#[cfg(feature = "smp")]
			if core_id == core_scheduler().core_id {
				let clone_task = Rc::new(RefCell::new(Task::from(clone_task)));
				core_scheduler().ready_queue.push(clone_task);
				false
			} else {
				input_locked.new_tasks.push_back(clone_task);
				true
			}
			#[cfg(not(feature = "smp"))]
			if core_id == 0 {
				let clone_task = Rc::new(RefCell::new(Task::from(clone_task)));
				core_scheduler().ready_queue.push(clone_task);
				false
			} else {
				panic!("Invalid core_id {core_id}!");
			}
		};

		// Wake up the CPU
		if wakeup {
			kernel::wakeup_core(core_id);
		}

		tid
	}

	#[cfg(feature = "newlib")]
	pub fn clone(&self, func: extern "C" fn(usize), arg: usize) -> TaskId {
		without_interrupts(|| self.clone_impl(func, arg))
	}

	/// Returns `true` if a reschedule is required
	#[inline]
	#[cfg(all(any(target_arch = "x86_64", target_arch = "riscv64"), feature = "smp"))]
	pub fn is_scheduling(&self) -> bool {
		self.current_task.borrow().prio < self.ready_queue.get_highest_priority()
	}

	#[inline]
	pub fn handle_waiting_tasks(&mut self) {
		without_interrupts(|| {
			crate::executor::run();
			self.blocked_tasks
				.handle_waiting_tasks(&mut self.ready_queue);
		});
	}

	#[cfg(not(feature = "smp"))]
	pub fn custom_wakeup(&mut self, task: TaskHandle) {
		without_interrupts(|| {
			let task = self.blocked_tasks.custom_wakeup(task);
			self.ready_queue.push(task);
		});
	}

	#[cfg(feature = "smp")]
	pub fn custom_wakeup(&mut self, task: TaskHandle) {
		if task.get_core_id() == self.core_id {
			without_interrupts(|| {
				let task = self.blocked_tasks.custom_wakeup(task);
				self.ready_queue.push(task);
			});
		} else {
			get_scheduler_input(task.get_core_id())
				.lock()
				.wakeup_tasks
				.push_back(task);
			// Wake up the CPU
			kernel::wakeup_core(task.get_core_id());
		}
	}

	#[inline]
	pub fn block_current_task(&mut self, wakeup_time: Option<u64>) {
		without_interrupts(|| {
			self.blocked_tasks
				.add(self.current_task.clone(), wakeup_time);
		});
	}

	#[inline]
	pub fn get_current_task_handle(&self) -> TaskHandle {
		without_interrupts(|| {
			let current_task_borrowed = self.current_task.borrow();

			TaskHandle::new(
				current_task_borrowed.id,
				current_task_borrowed.prio,
				#[cfg(feature = "smp")]
				current_task_borrowed.core_id,
			)
		})
	}

	#[inline]
	pub fn get_current_task_id(&self) -> TaskId {
		without_interrupts(|| self.current_task.borrow().id)
	}

	/// Returns the Rc<RefCell<Task>> for the currently running task.
	#[cfg(feature = "common-os")]
	#[inline]
	pub fn get_current_task(&self) -> Rc<RefCell<Task>> {
		self.current_task.clone()
	}

	#[cfg(feature = "common-os")]
	#[inline]
	pub fn set_current_task_object_map(
		&mut self,
		object_map: Arc<RwSpinLock<HashMap<RawFd, Arc<async_lock::RwLock<Fd>>, RandomState>>>,
	) {
		without_interrupts(|| self.current_task.borrow_mut().object_map = object_map);
	}

	#[inline]
	pub fn get_current_task_object_map(
		&self,
	) -> Arc<RwSpinLock<HashMap<RawFd, Arc<async_lock::RwLock<Fd>>, RandomState>>> {
		without_interrupts(|| self.current_task.borrow().object_map.clone())
	}

	/// Map a file descriptor to their IO interface and returns
	/// the shared reference
	#[inline]
	pub fn get_object(&self, fd: RawFd) -> io::Result<Arc<async_lock::RwLock<Fd>>> {
		without_interrupts(|| {
			let current_task = self.current_task.borrow();
			let object_map = current_task.object_map.read();
			object_map.get(&fd).cloned().ok_or(Errno::Badf)
		})
	}

	/// Creates a new map between file descriptor and their IO interface and
	/// clone the standard descriptors.
	#[cfg(feature = "common-os")]
	#[cfg_attr(
		not(any(
			target_arch = "x86_64",
			target_arch = "aarch64",
			target_arch = "riscv64"
		)),
		expect(dead_code)
	)]
	pub fn recreate_objmap(&self) -> io::Result<()> {
		let mut map = HashMap::<RawFd, Arc<async_lock::RwLock<Fd>>, RandomState>::with_hasher(
			RandomState::with_seeds(0, 0, 0, 0),
		);

		without_interrupts(|| {
			let mut current_task = self.current_task.borrow_mut();
			let object_map = current_task.object_map.read();

			// clone standard file descriptors
			for i in 0..3 {
				if let Some(obj) = object_map.get(&i) {
					map.insert(i, obj.clone());
				}
			}

			drop(object_map);
			current_task.object_map = Arc::new(RwSpinLock::new(map));
		});

		Ok(())
	}

	/// Insert a new IO interface and returns a file descriptor as
	/// identifier to this object
	pub fn insert_object(&self, obj: Arc<async_lock::RwLock<Fd>>) -> io::Result<RawFd> {
		without_interrupts(|| {
			let current_task = self.current_task.borrow();
			let mut object_map = current_task.object_map.write();

			let new_fd = || -> io::Result<RawFd> {
				let mut fd: RawFd = 0;
				loop {
					if !object_map.contains_key(&fd) {
						break Ok(fd);
					} else if fd == RawFd::MAX {
						break Err(Errno::Overflow);
					}

					fd = fd.saturating_add(1);
				}
			};

			let fd = new_fd()?;
			object_map.insert(fd, obj.clone());
			Ok(fd)
		})
	}

	/// Duplicate a IO interface and returns a new file descriptor as
	/// identifier to the new copy
	pub fn dup_object(&self, fd: RawFd) -> io::Result<RawFd> {
		without_interrupts(|| {
			let current_task = self.current_task.borrow();
			let mut object_map = current_task.object_map.write();

			let obj = (*(object_map.get(&fd).ok_or(Errno::Inval)?)).clone();

			let new_fd = || -> io::Result<RawFd> {
				let mut fd: RawFd = 0;
				loop {
					if !object_map.contains_key(&fd) {
						break Ok(fd);
					} else if fd == RawFd::MAX {
						break Err(Errno::Overflow);
					}

					fd = fd.saturating_add(1);
				}
			};

			let fd = new_fd()?;
			match object_map.entry(fd) {
				hash_map::Entry::Occupied(_occupied_entry) => Err(Errno::Mfile),
				hash_map::Entry::Vacant(vacant_entry) => {
					vacant_entry.insert(obj);
					Ok(fd)
				}
			}
		})
	}

	pub fn dup_object2(&self, fd1: RawFd, fd2: RawFd) -> io::Result<RawFd> {
		without_interrupts(|| {
			let current_task = self.current_task.borrow();
			let mut object_map = current_task.object_map.write();

			let obj = object_map.get(&fd1).cloned().ok_or(Errno::Badf)?;

			match object_map.entry(fd2) {
				hash_map::Entry::Occupied(_occupied_entry) => Err(Errno::Mfile),
				hash_map::Entry::Vacant(vacant_entry) => {
					vacant_entry.insert(obj);
					Ok(fd2)
				}
			}
		})
	}

	/// Remove a IO interface, which is named by the file descriptor
	pub fn remove_object(&self, fd: RawFd) -> io::Result<Arc<async_lock::RwLock<Fd>>> {
		without_interrupts(|| {
			let current_task = self.current_task.borrow();
			let mut object_map = current_task.object_map.write();

			object_map.remove(&fd).ok_or(Errno::Badf)
		})
	}

	#[inline]
	pub fn get_current_task_prio(&self) -> Priority {
		without_interrupts(|| self.current_task.borrow().prio)
	}

	/// Returns reference to prio_bitmap
	#[allow(dead_code)]
	#[inline]
	pub fn get_priority_bitmap(&self) -> &u64 {
		self.ready_queue.get_priority_bitmap()
	}

	#[cfg(target_arch = "x86_64")]
	pub fn set_current_kernel_stack(&self) {
		let current_task_borrowed = self.current_task.borrow();
		let tss = unsafe { &mut *CoreLocal::get().tss.get() };

		let rsp = current_task_borrowed.stacks.get_kernel_stack()
			+ current_task_borrowed.stacks.get_kernel_stack_size() as u64
			- TaskStacks::MARKER_SIZE as u64;
		tss.privilege_stack_table[0] = rsp.into();
		CoreLocal::get().kernel_stack.set(rsp.as_mut_ptr());
		let ist_start = current_task_borrowed.stacks.get_interrupt_stack()
			+ current_task_borrowed.stacks.get_interrupt_stack_size() as u64
			- TaskStacks::MARKER_SIZE as u64;
		tss.interrupt_stack_table[0] = ist_start.into();
	}

	pub fn set_current_task_priority(&mut self, prio: Priority) {
		without_interrupts(|| {
			trace!("Change priority of the current task");
			self.current_task.borrow_mut().prio = prio;
		});
	}

	pub fn set_priority(&mut self, id: TaskId, prio: Priority) -> Result<(), ()> {
		trace!("Change priority of task {id} to priority {prio}");

		without_interrupts(|| {
			let task = get_task_handle(id).ok_or(())?;
			#[cfg(feature = "smp")]
			let other_core = task.get_core_id() != self.core_id;
			#[cfg(not(feature = "smp"))]
			let other_core = false;

			if other_core {
				warn!("Have to change the priority on another core");
			} else if self.current_task.borrow().id == task.get_id() {
				self.current_task.borrow_mut().prio = prio;
			} else {
				self.ready_queue
					.set_priority(task, prio)
					.expect("Do not find valid task in ready queue");
			}

			Ok(())
		})
	}

	#[cfg(target_arch = "riscv64")]
	pub fn set_current_kernel_stack(&self) {
		let current_task_borrowed = self.current_task.borrow();

		let stack = (current_task_borrowed.stacks.get_kernel_stack()
			+ current_task_borrowed.stacks.get_kernel_stack_size() as u64
			- TaskStacks::MARKER_SIZE as u64)
			.as_u64();
		CoreLocal::get().kernel_stack.set(stack);
	}

	/// Save the FPU context for the current FPU owner and restore it for the current task,
	/// which wants to use the FPU now.
	#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
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
	fn cleanup_tasks(&mut self) {
		// Pop the first finished task and remove it from the TASKS list, which implicitly deallocates all associated memory.
		while let Some(finished_task) = self.finished_tasks.pop_front() {
			let id = finished_task.borrow().id;
			drop(finished_task);
			#[cfg(feature = "common-os")]
			trace!(
				"Cleaned up task {id} — free frames: {} KiB",
				crate::mm::FrameAlloc::free_space() >> 10
			);
			#[cfg(not(all(target_arch = "x86_64", feature = "common-os")))]
			debug!("Cleaned up task {id}");
		}
	}

	#[cfg(feature = "smp")]
	pub fn check_input(&mut self) {
		let mut input_locked = CoreLocal::get().scheduler_input.lock();

		while let Some(task) = input_locked.wakeup_tasks.pop_front() {
			let task = self.blocked_tasks.custom_wakeup(task);
			self.ready_queue.push(task);
		}

		while let Some(new_task) = input_locked.new_tasks.pop_front() {
			let task = Rc::new(RefCell::new(Task::from(new_task)));
			self.ready_queue.push(task.clone());
		}
	}

	/// Only the idle task should call this function.
	/// Set the idle task to halt state if not another
	/// available.
	pub fn run() -> ! {
		let backoff = Backoff::new();

		loop {
			let core_scheduler = core_scheduler();
			interrupts::disable();

			// run async tasks
			crate::executor::run();

			// do housekeeping
			#[cfg(feature = "smp")]
			core_scheduler.check_input();
			core_scheduler.cleanup_tasks();

			if core_scheduler.ready_queue.is_empty() {
				if backoff.is_completed() {
					interrupts::enable_and_wait();
					backoff.reset();
				} else {
					interrupts::enable();
					backoff.snooze();
				}
			} else {
				interrupts::enable();
				core_scheduler.reschedule();
				backoff.reset();
			}
		}
	}

	#[inline]
	#[cfg(target_arch = "aarch64")]
	pub fn get_last_stack_pointer(&self) -> VirtAddr {
		self.current_task.borrow().last_stack_pointer
	}

	/// Triggers the scheduler to reschedule the tasks.
	/// Interrupt flag must be cleared before calling this function.
	pub fn scheduler(&mut self) -> Option<*mut usize> {
		// run background tasks
		crate::executor::run();

		// Someone wants to give up the CPU
		// => we have time to cleanup the system
		self.cleanup_tasks();

		// Get information about the current task.
		let (id, last_stack_pointer, prio, status) = {
			let mut borrowed = self.current_task.borrow_mut();
			(
				borrowed.id,
				ptr::from_mut(&mut borrowed.last_stack_pointer).cast::<usize>(),
				borrowed.prio,
				borrowed.status,
			)
		};

		let mut new_task = None;

		if status == TaskStatus::Running {
			// A task is currently running.
			// Check if a task with a equal or higher priority is available.
			if let Some(task) = self.ready_queue.pop_with_prio(prio) {
				new_task = Some(task);
			}
		} else {
			if status == TaskStatus::Finished {
				// Mark the finished task as invalid and add it to the finished tasks for a later cleanup.
				self.current_task.borrow_mut().status = TaskStatus::Invalid;
				self.finished_tasks.push_back(self.current_task.clone());
			}

			// No task is currently running.
			// Check if there is any available task and get the one with the highest priority.
			if let Some(task) = self.ready_queue.pop() {
				// This available task becomes the new task.
				debug!("Task is available.");
				new_task = Some(task);
			} else if status != TaskStatus::Idle {
				// The Idle task becomes the new task.
				debug!("Only Idle Task is available.");
				new_task = Some(self.idle_task.clone());
			}
		}

		let task = new_task?;
		// There is a new task we want to switch to.

		// Handle the current task.
		if status == TaskStatus::Running {
			// Mark the running task as ready again and add it back to the queue.
			self.current_task.borrow_mut().status = TaskStatus::Ready;
			self.ready_queue.push(self.current_task.clone());
		}

		// Handle the new task and get information about it.
		let (new_id, new_stack_pointer) = {
			let mut borrowed = task.borrow_mut();
			if borrowed.status != TaskStatus::Idle {
				// Mark the new task as running.
				borrowed.status = TaskStatus::Running;
			}

			(borrowed.id, borrowed.last_stack_pointer)
		};

		if id == new_id {
			return None;
		}

		// Tell the scheduler about the new task.
		debug!(
			"Switching task from {} to {} (stack {:#X} => {:p})",
			id,
			new_id,
			unsafe { *last_stack_pointer },
			new_stack_pointer
		);
		#[cfg(not(target_arch = "riscv64"))]
		{
			self.current_task = task;
		}

		// Finally return the context of the new task.
		#[cfg(not(target_arch = "riscv64"))]
		return Some(last_stack_pointer);

		#[cfg(target_arch = "riscv64")]
		{
			if sstatus::read().fs() == sstatus::FS::Dirty {
				self.current_task.borrow_mut().last_fpu_state.save();
			}
			task.borrow().last_fpu_state.restore();

			// Install the new task's address space before switching to
			// its kernel stack.
			#[cfg(feature = "common-os")]
			{
				use riscv::register::satp;

				let new_ppn = task.borrow().root_page_table.as_usize() >> 12;
				if satp::read().ppn() != new_ppn {
					unsafe {
						satp::set(satp::Mode::Sv39, 0, new_ppn);
						riscv::asm::sfence_vma_all();
					}
				}
			}

			self.current_task = task;
			unsafe {
				switch_to_task(last_stack_pointer, new_stack_pointer.as_usize());
			}
			None
		}
	}
}

fn get_tid() -> TaskId {
	static TID_COUNTER: AtomicI32 = AtomicI32::new(0);
	let guard = TASKS.lock();

	loop {
		let id = TaskId::from(TID_COUNTER.fetch_add(1, Ordering::SeqCst));
		if !guard.contains_key(&id) {
			return id;
		}
	}
}

#[inline]
pub(crate) fn abort() -> ! {
	core_scheduler().exit(-1)
}

/// Add a per-core scheduler for the current core.
pub(crate) fn add_current_core() {
	// Create an idle task for this core.
	let core_id = core_id();
	let tid = get_tid();
	let idle_task = Rc::new(RefCell::new(Task::new_idle(tid, core_id)));

	// Add the ID -> Task mapping.
	WAITING_TASKS.lock().insert(tid, VecDeque::with_capacity(1));
	TASKS.lock().insert(
		tid,
		TaskHandle::new(
			tid,
			IDLE_PRIO,
			#[cfg(feature = "smp")]
			core_id,
		),
	);
	// Initialize a scheduler for this core.
	debug!("Initializing scheduler for core {core_id} with idle task {tid}");
	let boxed_scheduler = Box::new(PerCoreScheduler {
		#[cfg(feature = "smp")]
		core_id,
		current_task: idle_task.clone(),
		#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
		fpu_owner: idle_task.clone(),
		idle_task,
		ready_queue: PriorityTaskQueue::new(),
		finished_tasks: VecDeque::new(),
		blocked_tasks: BlockedTaskQueue::new(),
		timers: TimerList::new(),
	});

	let scheduler = Box::into_raw(boxed_scheduler);
	set_core_scheduler(scheduler);
	#[cfg(feature = "smp")]
	{
		SCHEDULER_INPUTS.lock().insert(
			core_id.try_into().unwrap(),
			&CoreLocal::get().scheduler_input,
		);
	}
}

#[inline]
#[cfg(feature = "smp")]
fn get_scheduler_input(core_id: CoreId) -> &'static InterruptTicketMutex<SchedulerInput> {
	SCHEDULER_INPUTS.lock()[usize::try_from(core_id).unwrap()]
}

pub unsafe fn spawn(
	func: unsafe extern "C" fn(usize),
	arg: usize,
	prio: Priority,
	stack_size: usize,
	selector: isize,
) -> TaskId {
	let core_id = if selector < 0 {
		// use Round Robin to schedule the cores
		SPAWN_COUNTER.fetch_add(1, Ordering::SeqCst) % get_processor_count()
	} else {
		selector as u32
	};

	unsafe { PerCoreScheduler::spawn(func, arg, prio, core_id, stack_size) }
}

/// Spawn a user-space thread that shares the current task's address space.
///
/// Used by `sys_spawn`/`sys_spawn2` under the `common-os` feature to
/// implement POSIX-style threads: the entry point `func` lives in user
/// space and the new thread executes in ring 3 against the parent
/// process's root page table.
#[cfg(feature = "common-os")]
pub unsafe fn spawn_thread(
	func: unsafe extern "C" fn(usize),
	arg: usize,
	prio: Priority,
	stack_size: usize,
	selector: isize,
) -> TaskId {
	let core_id = if selector < 0 {
		SPAWN_COUNTER.fetch_add(1, Ordering::SeqCst) % get_processor_count()
	} else {
		selector as u32
	};

	unsafe { PerCoreScheduler::spawn_thread(func, arg, prio, core_id, stack_size) }
}

#[allow(clippy::result_unit_err)]
pub fn join(id: TaskId) -> Result<(), ()> {
	let core_scheduler = core_scheduler();

	debug!(
		"Task {} is waiting for task {}",
		core_scheduler.get_current_task_id(),
		id
	);

	loop {
		let mut waiting_tasks_guard = WAITING_TASKS.lock();

		let Some(queue) = waiting_tasks_guard.get_mut(&id) else {
			return Ok(());
		};

		queue.push_back(core_scheduler.get_current_task_handle());
		core_scheduler.block_current_task(None);

		// Switch to the next task.
		drop(waiting_tasks_guard);
		core_scheduler.reschedule();
	}
}

/// Fork the current task.
///
/// Marks user pages as Copy-On-Write, duplicates the page table hierarchy,
/// copies the kernel stack, and enqueues a new child task that will resume
/// execution right after the `prepare_fork_child_stack` call returning 1.
///
/// Returns the child's `TaskId` in the parent; the child itself sees `TaskId(0)`.
#[cfg(all(
	any(target_arch = "x86_64", target_arch = "aarch64"),
	all(feature = "common-os", feature = "fork")
))]
pub unsafe fn fork() -> TaskId {
	use crate::arch::kernel::prepare_fork_child_stack;
	use crate::arch::mm::prepare_mem_copy_on_write;

	let core_id = SPAWN_COUNTER.fetch_add(1, Ordering::SeqCst) % get_processor_count();

	// Mark user pages COW before copying the page table.
	prepare_mem_copy_on_write();

	let stack_size = core_scheduler()
		.get_current_task()
		.borrow()
		.stacks
		.get_user_stack_size();
	let stacks = TaskStacks::new(stack_size);

	let mut child_stack_pointer: usize = 0;
	let mut child_root_page_table: usize = 0;

	// Copy the kernel stack and duplicate the page table; returns false in parent.
	let is_child = unsafe {
		prepare_fork_child_stack(
			&raw mut child_stack_pointer,
			&raw mut child_root_page_table,
			stacks.get_stack_virt_addr().as_usize(),
		)
	};

	if is_child {
		// We are in the child context (stack is already switched).
		// Prevent the newly-allocated stacks from being dropped here —
		// they are owned by the child task created in the parent.
		core::mem::forget(stacks);
		return TaskId::from(0);
	}

	// Parent path: register the child task.
	let tid = get_tid();
	let child_last_sp = VirtAddr::new(child_stack_pointer.try_into().unwrap());
	let parent_user_sp = core_scheduler()
		.get_current_task()
		.borrow()
		.user_stack_pointer;
	let parent_prio = core_scheduler().get_current_task().borrow().prio;
	let parent_object_map = Arc::new(RwSpinLock::new(HashMap::<
		RawFd,
		Arc<async_lock::RwLock<Fd>>,
		RandomState,
	>::with_hasher(RandomState::with_seeds(
		0, 0, 0, 0,
	))));
	for (key, val) in core_scheduler().get_current_task_object_map().read().iter() {
		parent_object_map.write().insert(*key, val.clone());
	}
	let parent_tls_template = core_scheduler()
		.get_current_task()
		.borrow()
		.tls_template
		.clone();
	let parent_vmas = Arc::new(RwSpinLock::new(
		core_scheduler()
			.get_current_task()
			.borrow()
			.vmas
			.read()
			.clone(),
	));

	let child_task = Task::new_fork(
		tid,
		core_id,
		TaskStatus::Ready,
		parent_prio,
		stacks,
		child_last_sp,
		parent_user_sp,
		parent_object_map,
		Arc::new(RootPageTable::new(child_root_page_table)),
		parent_tls_template,
		parent_vmas,
	);

	let wakeup = {
		#[cfg(feature = "smp")]
		let _input_locked = get_scheduler_input(core_id).lock();
		WAITING_TASKS.lock().insert(tid, VecDeque::with_capacity(1));
		TASKS.lock().insert(
			tid,
			TaskHandle::new(
				tid,
				parent_prio,
				#[cfg(feature = "smp")]
				core_id,
			),
		);
		NO_TASKS.fetch_add(1, Ordering::SeqCst);

		#[cfg(feature = "smp")]
		if core_id == core_scheduler().core_id {
			let task = Rc::new(RefCell::new(child_task));
			core_scheduler().ready_queue.push(task);
			false
		} else {
			// For SMP we'd need to send to the target core; for now push locally.
			let task = Rc::new(RefCell::new(child_task));
			core_scheduler().ready_queue.push(task);
			false
		}
		#[cfg(not(feature = "smp"))]
		{
			let task = Rc::new(RefCell::new(child_task));
			core_scheduler().ready_queue.push(task);
			false
		}
	};

	if wakeup {
		kernel::wakeup_core(core_id);
	}

	debug!("Child was created and has the id {tid}");

	tid
}

pub fn shutdown(arg: i32) -> ! {
	crate::syscalls::shutdown(arg)
}

fn get_task_handle(id: TaskId) -> Option<TaskHandle> {
	TASKS.lock().get(&id).copied()
}

#[cfg(feature = "common-os")]
pub(crate) static BOOT_ROOT_PAGE_TABLE: OnceCell<usize> = OnceCell::new();

#[cfg(all(target_arch = "x86_64", feature = "common-os"))]
pub(crate) fn get_root_page_table() -> usize {
	let current_task_borrowed = core_scheduler().current_task.borrow_mut();
	current_task_borrowed.root_page_table.as_usize()
}
