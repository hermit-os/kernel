use alloc::collections::BTreeMap;
use core::isize;
#[cfg(feature = "newlib")]
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::{AtomicU32, Ordering};

use hermit_sync::InterruptTicketMutex;

use crate::arch::core_local::*;
use crate::arch::get_processor_count;
use crate::arch::processor::{get_frequency, get_timestamp};
use crate::config::USER_STACK_SIZE;
use crate::errno::*;
#[cfg(feature = "newlib")]
use crate::mm::{task_heap_end, task_heap_start};
use crate::scheduler::task::{Priority, TaskHandle, TaskId};
use crate::syscalls::timer::timespec;
use crate::{arch, scheduler, syscalls};

#[cfg(feature = "newlib")]
pub type SignalHandler = extern "C" fn(i32);
pub type Tid = u32;

extern "C" fn __sys_getpid() -> Tid {
	core_scheduler().get_current_task_id().into()
}

#[no_mangle]
pub extern "C" fn sys_getpid() -> Tid {
	kernel_function!(__sys_getpid())
}

#[cfg(feature = "newlib")]
extern "C" fn __sys_getprio(id: *const Tid) -> i32 {
	let task = core_scheduler().get_current_task_handle();

	if id.is_null() || unsafe { *id } == task.get_id().into() {
		i32::from(task.get_priority().into())
	} else {
		-EINVAL
	}
}

#[cfg(feature = "newlib")]
#[no_mangle]
pub extern "C" fn sys_getprio(id: *const Tid) -> i32 {
	kernel_function!(__sys_getprio(id))
}

#[cfg(feature = "newlib")]
#[no_mangle]
pub extern "C" fn sys_setprio(_id: *const Tid, _prio: i32) -> i32 {
	-ENOSYS
}

extern "C" fn __sys_exit(arg: i32) -> ! {
	debug!("Exit program with error code {}!", arg);
	syscalls::__sys_shutdown(arg)
}

#[no_mangle]
pub extern "C" fn sys_exit(arg: i32) -> ! {
	kernel_function!(__sys_exit(arg))
}

extern "C" fn __sys_thread_exit(arg: i32) -> ! {
	debug!("Exit thread with error code {}!", arg);
	core_scheduler().exit(arg)
}

#[no_mangle]
pub extern "C" fn sys_thread_exit(arg: i32) -> ! {
	kernel_function!(__sys_thread_exit(arg))
}

#[no_mangle]
pub extern "C" fn sys_abort() -> ! {
	kernel_function!(__sys_exit(-1))
}

#[cfg(feature = "newlib")]
static SBRK_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "newlib")]
pub fn sbrk_init() {
	SBRK_COUNTER.store(task_heap_start().as_usize(), Ordering::SeqCst);
}

#[cfg(feature = "newlib")]
extern "C" fn __sys_sbrk(incr: isize) -> usize {
	// Get the boundaries of the task heap and verify that they are suitable for sbrk.
	let task_heap_start = task_heap_start();
	let task_heap_end = task_heap_end();
	let old_end;

	if incr >= 0 {
		old_end = SBRK_COUNTER.fetch_add(incr as usize, Ordering::SeqCst);
		assert!(task_heap_end.as_usize() >= old_end + incr as usize);
	} else {
		old_end = SBRK_COUNTER.fetch_sub(incr.unsigned_abs(), Ordering::SeqCst);
		assert!(task_heap_start.as_usize() < old_end - incr.unsigned_abs());
	}

	old_end
}

#[cfg(feature = "newlib")]
#[no_mangle]
pub extern "C" fn sys_sbrk(incr: isize) -> usize {
	kernel_function!(__sys_sbrk(incr))
}

pub(crate) extern "C" fn __sys_usleep(usecs: u64) {
	if usecs >= 10_000 {
		// Enough time to set a wakeup timer and block the current task.
		debug!("sys_usleep blocking the task for {} microseconds", usecs);
		let wakeup_time = arch::processor::get_timer_ticks() + usecs;
		let core_scheduler = core_scheduler();
		core_scheduler.block_current_task(Some(wakeup_time));

		// Switch to the next task.
		core_scheduler.reschedule();
	} else if usecs > 0 {
		// Not enough time to set a wakeup timer, so just do busy-waiting.
		let end = arch::processor::get_timestamp() + u64::from(get_frequency()) * usecs;
		while get_timestamp() < end {
			__sys_yield();
		}
	}
}

#[no_mangle]
pub extern "C" fn sys_usleep(usecs: u64) {
	kernel_function!(__sys_usleep(usecs))
}

#[no_mangle]
pub extern "C" fn sys_msleep(ms: u32) {
	kernel_function!(__sys_usleep(u64::from(ms) * 1000))
}

extern "C" fn __sys_nanosleep(rqtp: *const timespec, _rmtp: *mut timespec) -> i32 {
	assert!(
		!rqtp.is_null(),
		"sys_nanosleep called with a zero rqtp parameter"
	);
	let requested_time = unsafe { &*rqtp };
	if requested_time.tv_sec < 0
		|| requested_time.tv_nsec < 0
		|| requested_time.tv_nsec > 999_999_999
	{
		debug!("sys_nanosleep called with an invalid requested time, returning -EINVAL");
		return -EINVAL;
	}

	let microseconds =
		(requested_time.tv_sec as u64) * 1_000_000 + (requested_time.tv_nsec as u64) / 1_000;
	__sys_usleep(microseconds);

	0
}

#[no_mangle]
pub extern "C" fn sys_nanosleep(rqtp: *const timespec, rmtp: *mut timespec) -> i32 {
	kernel_function!(__sys_nanosleep(rqtp, rmtp))
}

#[cfg(feature = "newlib")]
extern "C" fn __sys_clone(id: *mut Tid, func: extern "C" fn(usize), arg: usize) -> i32 {
	let task_id = core_scheduler().clone(func, arg);

	if !id.is_null() {
		unsafe {
			*id = task_id.into() as u32;
		}
	}

	0
}

/// Creates a new thread based on the configuration of the current thread.
#[cfg(feature = "newlib")]
#[no_mangle]
pub extern "C" fn sys_clone(id: *mut Tid, func: extern "C" fn(usize), arg: usize) -> i32 {
	kernel_function!(__sys_clone(id, func, arg))
}

extern "C" fn __sys_yield() {
	core_scheduler().reschedule();
}

#[no_mangle]
pub extern "C" fn sys_yield() {
	kernel_function!(__sys_yield())
}

#[cfg(feature = "newlib")]
extern "C" fn __sys_kill(dest: Tid, signum: i32) -> i32 {
	debug!(
		"sys_kill is unimplemented, returning -ENOSYS for killing {} with signal {}",
		dest, signum
	);
	-ENOSYS
}

#[cfg(feature = "newlib")]
#[no_mangle]
pub extern "C" fn sys_kill(dest: Tid, signum: i32) -> i32 {
	kernel_function!(__sys_kill(dest, signum))
}

#[cfg(feature = "newlib")]
extern "C" fn __sys_signal(_handler: SignalHandler) -> i32 {
	debug!("sys_signal is unimplemented");
	0
}

#[cfg(feature = "newlib")]
#[no_mangle]
pub extern "C" fn sys_signal(handler: SignalHandler) -> i32 {
	kernel_function!(__sys_signal(handler))
}

extern "C" fn __sys_spawn2(
	func: extern "C" fn(usize),
	arg: usize,
	prio: u8,
	stack_size: usize,
	selector: isize,
) -> Tid {
	static CORE_COUNTER: AtomicU32 = AtomicU32::new(1);

	let core_id = if selector < 0 {
		// use Round Robin to schedule the cores
		CORE_COUNTER.fetch_add(1, Ordering::SeqCst) % get_processor_count()
	} else {
		selector as u32
	};

	scheduler::PerCoreScheduler::spawn(func, arg, Priority::from(prio), core_id, stack_size).into()
		as Tid
}

#[no_mangle]
pub extern "C" fn sys_spawn2(
	func: extern "C" fn(usize),
	arg: usize,
	prio: u8,
	stack_size: usize,
	selector: isize,
) -> Tid {
	kernel_function!(__sys_spawn2(func, arg, prio, stack_size, selector))
}

extern "C" fn __sys_spawn(
	id: *mut Tid,
	func: extern "C" fn(usize),
	arg: usize,
	prio: u8,
	selector: isize,
) -> i32 {
	let new_id = __sys_spawn2(func, arg, prio, USER_STACK_SIZE, selector);

	if !id.is_null() {
		unsafe {
			*id = new_id;
		}
	}

	0
}

#[no_mangle]
pub extern "C" fn sys_spawn(
	id: *mut Tid,
	func: extern "C" fn(usize),
	arg: usize,
	prio: u8,
	selector: isize,
) -> i32 {
	kernel_function!(__sys_spawn(id, func, arg, prio, selector))
}

extern "C" fn __sys_join(id: Tid) -> i32 {
	match scheduler::join(TaskId::from(id)) {
		Ok(()) => 0,
		_ => -EINVAL,
	}
}

#[no_mangle]
pub extern "C" fn sys_join(id: Tid) -> i32 {
	kernel_function!(__sys_join(id))
}

/// Mapping between blocked tasks and their TaskHandle
static BLOCKED_TASKS: InterruptTicketMutex<BTreeMap<TaskId, TaskHandle>> =
	InterruptTicketMutex::new(BTreeMap::new());

extern "C" fn __sys_block_current_task(timeout: &Option<u64>) {
	let wakeup_time = timeout.map(|t| arch::processor::get_timer_ticks() + t * 1000);
	let core_scheduler = core_scheduler();
	let handle = core_scheduler.get_current_task_handle();
	let tid = core_scheduler.get_current_task_id();

	BLOCKED_TASKS.lock().insert(tid, handle);
	core_scheduler.block_current_task(wakeup_time);
}

/// Set the current task state to `blocked`
#[no_mangle]
pub extern "C" fn sys_block_current_task() {
	kernel_function!(__sys_block_current_task(&None))
}

/// Set the current task state to `blocked`
#[no_mangle]
pub extern "C" fn sys_block_current_task_with_timeout(timeout: u64) {
	kernel_function!(__sys_block_current_task(&Some(timeout)))
}

extern "C" fn __sys_wakeup_task(id: Tid) {
	let task_id = TaskId::from(id);

	if let Some(handle) = BLOCKED_TASKS.lock().remove(&task_id) {
		core_scheduler().custom_wakeup(handle);
	}
}

/// Wake up the task with the identifier `id`
#[no_mangle]
pub extern "C" fn sys_wakeup_task(id: Tid) {
	kernel_function!(__sys_wakeup_task(id))
}

extern "C" fn __sys_get_priority() -> u8 {
	core_scheduler().get_current_task_prio().into()
}

/// Determine the priority of the current thread
#[no_mangle]
pub extern "C" fn sys_get_priority() -> u8 {
	kernel_function!(__sys_get_priority())
}

extern "C" fn __sys_set_priority(id: Tid, prio: u8) {
	if prio > 0 {
		core_scheduler()
			.set_priority(TaskId::from(id), Priority::from(prio))
			.expect("Unable to set priority");
	} else {
		panic!("Invalid priority {}", prio);
	}
}

/// Set priority of the thread with the identifier `id`
#[no_mangle]
pub extern "C" fn sys_set_priority(id: Tid, prio: u8) {
	kernel_function!(__sys_set_priority(id, prio))
}

extern "C" fn __sys_set_current_task_priority(prio: u8) {
	if prio > 0 {
		core_scheduler().set_current_task_priority(Priority::from(prio));
	} else {
		panic!("Invalid priority {}", prio);
	}
}

/// Set priority of the current thread
#[no_mangle]
pub extern "C" fn sys_set_current_task_priority(prio: u8) {
	kernel_function!(__sys_set_current_task_priority(prio))
}
