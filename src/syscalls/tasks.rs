use alloc::collections::BTreeMap;
#[cfg(feature = "newlib")]
use core::sync::atomic::{AtomicUsize, Ordering};

use hermit_sync::InterruptTicketMutex;

use crate::arch::core_local::*;
use crate::arch::processor::{get_frequency, get_timestamp};
use crate::config::USER_STACK_SIZE;
use crate::errno::*;
#[cfg(feature = "newlib")]
use crate::mm::{task_heap_end, task_heap_start};
use crate::scheduler::task::{Priority, TaskHandle, TaskId};
use crate::scheduler::PerCoreSchedulerExt;
use crate::time::timespec;
use crate::{arch, scheduler};

#[cfg(feature = "newlib")]
pub type SignalHandler = extern "C" fn(i32);
pub type Tid = i32;

#[hermit_macro::system]
#[no_mangle]
pub extern "C" fn sys_getpid() -> Tid {
	0
}

#[cfg(feature = "newlib")]
#[hermit_macro::system]
#[no_mangle]
pub extern "C" fn sys_getprio(id: *const Tid) -> i32 {
	let task = core_scheduler().get_current_task_handle();

	if id.is_null() || unsafe { *id } == task.get_id().into() {
		i32::from(task.get_priority().into())
	} else {
		-EINVAL
	}
}

#[cfg(feature = "newlib")]
#[hermit_macro::system]
#[no_mangle]
pub extern "C" fn sys_setprio(_id: *const Tid, _prio: i32) -> i32 {
	-ENOSYS
}

fn exit(arg: i32) -> ! {
	debug!("Exit program with error code {}!", arg);
	super::shutdown(arg)
}

#[hermit_macro::system]
#[no_mangle]
pub extern "C" fn sys_exit(status: i32) -> ! {
	exit(status)
}

#[hermit_macro::system]
#[no_mangle]
pub extern "C" fn sys_thread_exit(status: i32) -> ! {
	debug!("Exit thread with error code {}!", status);
	core_scheduler().exit(status)
}

#[hermit_macro::system]
#[no_mangle]
pub extern "C" fn sys_abort() -> ! {
	exit(-1)
}

#[cfg(feature = "newlib")]
static SBRK_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "newlib")]
pub fn sbrk_init() {
	SBRK_COUNTER.store(task_heap_start().as_usize(), Ordering::SeqCst);
}

#[cfg(feature = "newlib")]
#[hermit_macro::system]
#[no_mangle]
pub extern "C" fn sys_sbrk(incr: isize) -> usize {
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

pub(super) fn usleep(usecs: u64) {
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
			core_scheduler().reschedule();
		}
	}
}

#[hermit_macro::system]
#[no_mangle]
pub extern "C" fn sys_msleep(ms: u32) {
	usleep(u64::from(ms) * 1000)
}

#[hermit_macro::system]
#[no_mangle]
pub extern "C" fn sys_usleep(usecs: u64) {
	usleep(usecs)
}

#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_nanosleep(rqtp: *const timespec, _rmtp: *mut timespec) -> i32 {
	assert!(
		!rqtp.is_null(),
		"sys_nanosleep called with a zero rqtp parameter"
	);
	let requested_time = unsafe { &*rqtp };
	if requested_time.tv_sec < 0 || requested_time.tv_nsec > 999_999_999 {
		debug!("sys_nanosleep called with an invalid requested time, returning -EINVAL");
		return -EINVAL;
	}

	let microseconds =
		(requested_time.tv_sec as u64) * 1_000_000 + (requested_time.tv_nsec as u64) / 1_000;
	usleep(microseconds);

	0
}

/// Creates a new thread based on the configuration of the current thread.
#[cfg(feature = "newlib")]
#[hermit_macro::system]
#[no_mangle]
pub extern "C" fn sys_clone(id: *mut Tid, func: extern "C" fn(usize), arg: usize) -> i32 {
	let task_id = core_scheduler().clone(func, arg);

	if !id.is_null() {
		unsafe {
			*id = task_id.into();
		}
	}

	0
}

#[hermit_macro::system]
#[no_mangle]
pub extern "C" fn sys_yield() {
	core_scheduler().reschedule();
}

#[cfg(feature = "newlib")]
#[hermit_macro::system]
#[no_mangle]
pub extern "C" fn sys_kill(dest: Tid, signum: i32) -> i32 {
	debug!(
		"sys_kill is unimplemented, returning -ENOSYS for killing {} with signal {}",
		dest, signum
	);
	-ENOSYS
}

#[cfg(feature = "newlib")]
#[hermit_macro::system]
#[no_mangle]
pub extern "C" fn sys_signal(_handler: SignalHandler) -> i32 {
	debug!("sys_signal is unimplemented");
	0
}

#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_spawn2(
	func: unsafe extern "C" fn(usize),
	arg: usize,
	prio: u8,
	stack_size: usize,
	selector: isize,
) -> Tid {
	unsafe { scheduler::spawn(func, arg, Priority::from(prio), stack_size, selector).into() }
}

#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_spawn(
	id: *mut Tid,
	func: unsafe extern "C" fn(usize),
	arg: usize,
	prio: u8,
	selector: isize,
) -> i32 {
	let new_id = unsafe {
		scheduler::spawn(func, arg, Priority::from(prio), USER_STACK_SIZE, selector).into()
	};

	if !id.is_null() {
		unsafe {
			*id = new_id;
		}
	}

	0
}

#[hermit_macro::system]
#[no_mangle]
pub extern "C" fn sys_join(id: Tid) -> i32 {
	match scheduler::join(TaskId::from(id)) {
		Ok(()) => 0,
		_ => -EINVAL,
	}
}

/// Mapping between blocked tasks and their TaskHandle
static BLOCKED_TASKS: InterruptTicketMutex<BTreeMap<TaskId, TaskHandle>> =
	InterruptTicketMutex::new(BTreeMap::new());

fn block_current_task(timeout: &Option<u64>) {
	let wakeup_time = timeout.map(|t| arch::processor::get_timer_ticks() + t * 1000);
	let core_scheduler = core_scheduler();
	let handle = core_scheduler.get_current_task_handle();
	let tid = core_scheduler.get_current_task_id();

	BLOCKED_TASKS.lock().insert(tid, handle);
	core_scheduler.block_current_task(wakeup_time);
}

/// Set the current task state to `blocked`
#[hermit_macro::system]
#[no_mangle]
pub extern "C" fn sys_block_current_task() {
	block_current_task(&None)
}

/// Set the current task state to `blocked`
#[hermit_macro::system]
#[no_mangle]
pub extern "C" fn sys_block_current_task_with_timeout(timeout: u64) {
	block_current_task(&Some(timeout))
}

/// Wake up the task with the identifier `id`
#[hermit_macro::system]
#[no_mangle]
pub extern "C" fn sys_wakeup_task(id: Tid) {
	let task_id = TaskId::from(id);

	if let Some(handle) = BLOCKED_TASKS.lock().remove(&task_id) {
		core_scheduler().custom_wakeup(handle);
	}
}

/// Determine the priority of the current thread
#[hermit_macro::system]
#[no_mangle]
pub extern "C" fn sys_get_priority() -> u8 {
	core_scheduler().get_current_task_prio().into()
}

/// Set priority of the thread with the identifier `id`
#[hermit_macro::system]
#[no_mangle]
pub extern "C" fn sys_set_priority(id: Tid, prio: u8) {
	if prio > 0 {
		core_scheduler()
			.set_priority(TaskId::from(id), Priority::from(prio))
			.expect("Unable to set priority");
	} else {
		panic!("Invalid priority {}", prio);
	}
}

/// Set priority of the current thread
#[hermit_macro::system]
#[no_mangle]
pub extern "C" fn sys_set_current_task_priority(prio: u8) {
	if prio > 0 {
		core_scheduler().set_current_task_priority(Priority::from(prio));
	} else {
		panic!("Invalid priority {}", prio);
	}
}
