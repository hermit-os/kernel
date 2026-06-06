use alloc::collections::linked_list::CursorMut;
use crate::arch::kernel::core_local::core_scheduler;
use crate::arch::kernel::processor::get_timer_ticks;
use crate::errno::Errno;
use crate::scheduler::task::{TaskHandle, TaskHandlePriorityQueue};
use crate::scheduler::PerCoreSchedulerExt;
use alloc::collections::LinkedList;
use core::sync::atomic::Ordering::SeqCst;
use core::sync::atomic::AtomicU32;
use hermit_sync::{SpinMutex, SpinMutexGuard};

struct BucketElem(usize, TaskHandlePriorityQueue);

type Bucket = SpinMutex<TaskListBucket>;

#[repr(transparent)]
struct TaskListBucket(LinkedList<BucketElem>);

struct WrappedCursor<'a>(CursorMut<'a, BucketElem>);

impl WrappedCursor<'_> {
	pub fn pop(&mut self) -> Option<TaskHandle> {
		match self.0.current() {
			None => None,
			Some(task_list) => {
				let task = task_list.1.pop();

				if task_list.1.is_empty() {
					self.0.remove_current();
				}

				task
			}
		}
	}
}

impl TaskListBucket {
	pub fn insert_task(&mut self, address: usize, handle: TaskHandle) {
		for elem in self.0.iter_mut() {
			if elem.0 == address {
				elem.1.push(handle);
				return;
			}
		}

		let mut task_list = TaskHandlePriorityQueue::new();
		task_list.push(handle);
		self.0.push_front(BucketElem(address, task_list));
	}

	pub fn contains_task(&self, address: usize, handle: TaskHandle) -> bool {
		for elem in self.0.iter() {
			if elem.0 == address {
				return elem.1.contains(handle);
			}
		}
		false
	}

	/// Removes a task from this bucket, and returns a boolean indicating if it was present.
	pub fn remove_task(&mut self, address: usize, task: TaskHandle) -> bool {
		let mut cursor = self.0.cursor_front_mut();
		while let Some(elem) = cursor.current() {
			if elem.0 == address {
				let was_present =elem.1.remove(task);

				if elem.1.is_empty() {
					cursor.remove_current();
				}

				return was_present;
			}
			cursor.move_next();
		}

		false
	}

	fn get_pop_list(&mut self, address: usize) -> Option<WrappedCursor<'_>> {
		let mut cursor = self.0.cursor_front_mut();
		while let Some(elem) = cursor.current() {
			if elem.0 == address {
				return Some(WrappedCursor(cursor));
			}
			cursor.move_next();
		}
		None
	}
}

struct BucketList<const N: usize>([Bucket; N]);

impl<const N: usize> BucketList<N> {
	pub const fn new() -> Self {
		Self([const { SpinMutex::new(TaskListBucket(LinkedList::new())) }; N])
	}

	fn hash_key(v: usize) -> usize {
		let v = (v >> 3).to_be_bytes();
		let hashed = seahash::hash(&v) as usize;
		hashed % N
	}

	pub fn lock_bucket(&self, address: usize) -> SpinMutexGuard<'_, TaskListBucket> {
		let bucket = Self::hash_key(address);
		self.0[bucket].lock()
	}
}

static PARKING_LOT: BucketList<64> = BucketList::new();

bitflags! {
	pub struct Flags: u32 {
		/// Use a relative timeout
		const RELATIVE = 0b01;
	}
}

#[inline(always)]
fn addr(addr: &AtomicU32) -> usize {
	let ptr: *const _ = addr;
	ptr.addr()
}

/// If the value at address matches the expected value, park the current thread until it is either
/// woken up with `futex_wake` (returns 0) or the specified timeout elapses (returns -ETIMEDOUT).
///
/// The timeout is given in microseconds. If [`Flags::RELATIVE`] is given, it is interpreted as
/// relative to the current time. Otherwise it is understood to be an absolute time
/// (see `get_timer_ticks`).
pub(crate) fn futex_wait(
	address: &AtomicU32,
	expected: u32,
	timeout: Option<u64>,
	flags: Flags,
) -> i32 {
	let address_usize = addr(address);
	let mut parking_lot = PARKING_LOT.lock_bucket(address_usize);
	// Check the futex value after locking the parking lot so that all changes are observed.
	if address.load(SeqCst) != expected {
		return -i32::from(Errno::Again);
	}

	let wakeup_time = if flags.contains(Flags::RELATIVE) {
		timeout.and_then(|t| get_timer_ticks().checked_add(t))
	} else {
		timeout
	};

	let scheduler = core_scheduler();
	scheduler.block_current_task(wakeup_time);
	let handle = scheduler.get_current_task_handle();
	parking_lot.insert_task(address_usize, handle);
	drop(parking_lot);

	loop {
		scheduler.reschedule();
		// Assume this will return immediately (no other task on core!)

		let mut parking_lot = PARKING_LOT.lock_bucket(address_usize);
		if matches!(wakeup_time, Some(t) if t <= get_timer_ticks()) {
			// Timeout occurred, try to remove ourselves from the waiting queue.
			let was_present = parking_lot.remove_task(address_usize, handle);

			return if was_present {
				-i32::from(Errno::Timedout)
			} else {
				// If we are not in the waking queue, this must have been a wakeup.
				0
			}
		} else {
			let is_in_queue = parking_lot.contains_task(address_usize, handle);

			if is_in_queue {
				// A spurious wakeup occurred, sleep again.
				// Tasks do not change core, so the handle in the parking lot is still current.
				scheduler.block_current_task(wakeup_time);
			} else {
				// If we are not in the waking queue, this must have been a wakeup.
				return 0;
			}
		}
		drop(parking_lot);
	}
}

/// If the value at address matches the expected value, park the current thread until it is either
/// woken up with `futex_wake` (returns 0) or the specified timeout elapses (returns -ETIMEDOUT).
/// In addition, the value `new_value` will stored at address.
///
/// The timeout is given in microseconds. If [`Flags::RELATIVE`] is given, it is interpreted as
/// relative to the current time. Otherwise it is understood to be an absolute time
/// (see `get_timer_ticks`).
pub(crate) fn futex_wait_and_set(
	address: &AtomicU32,
	expected: u32,
	timeout: Option<u64>,
	flags: Flags,
	new_value: u32,
) -> i32 {
	let address_usize = addr(address);
	let mut parking_lot = PARKING_LOT.lock_bucket(address_usize);
	// Check the futex value after locking the parking lot so that all changes are observed.
	if address.swap(new_value, SeqCst) != expected {
		return -i32::from(Errno::Again);
	}

	let wakeup_time = if flags.contains(Flags::RELATIVE) {
		timeout.and_then(|t| get_timer_ticks().checked_add(t))
	} else {
		timeout
	};

	let scheduler = core_scheduler();
	scheduler.block_current_task(wakeup_time);
	let handle = scheduler.get_current_task_handle();
	parking_lot.insert_task(address_usize, handle);
	drop(parking_lot);

	loop {
		scheduler.reschedule();

		let mut parking_lot = PARKING_LOT.lock_bucket(address_usize);
		if matches!(wakeup_time, Some(t) if t <= get_timer_ticks()) {
			// Timeout occurred, try to remove ourselves from the waiting queue.
			let was_present = parking_lot.remove_task(address_usize, handle);

			return if was_present {
				-i32::from(Errno::Timedout)
			} else {
				// If we are not in the waking queue, this must have been a wakeup.
				0
			}
		} else {
			let is_in_queue = parking_lot.contains_task(address_usize, handle);

			if is_in_queue {
				// A spurious wakeup occurred, sleep again.
				// Tasks do not change core, so the handle in the parking lot is still current.
				scheduler.block_current_task(wakeup_time);
			} else {
				// If we are not in the waking queue, this must have been a wakeup.
				return 0;
			}
		}
		drop(parking_lot);
	}
}

/// Wake `count` threads waiting on the futex at address. Returns the number of threads
/// woken up (saturates to `i32::MAX`). If `count` is `i32::MAX`, wake up all matching
/// waiting threads. If `count` is negative, returns -EINVAL.
/// `address` is used only for its address.
/// It is safe to pass a dangling pointer.
pub(crate) fn futex_wake(address: *const AtomicU32, count: i32) -> i32 {
	if count < 0 {
		return -i32::from(Errno::Inval);
	}

	let address_usize = address.addr();
	let mut parking_lot = PARKING_LOT.lock_bucket(address_usize);
	let Some(mut queue) = parking_lot.get_pop_list(address_usize) else {
		return 0;
	};

	let scheduler = core_scheduler();
	let mut woken = 0;
	while woken != count || count == i32::MAX {
		match queue.pop() {
			Some(handle) => scheduler.custom_wakeup(handle),
			None => break,
		}
		woken = woken.saturating_add(1);
	}

	woken
}

/// Wake `count` threads waiting on the futex at address. Returns the number of threads
/// woken up (saturates to `i32::MAX`). If `count` is `i32::MAX`, wake up all matching
/// waiting threads. If `count` is negative, returns -EINVAL. If no thread is available,
/// the futex at address will set to `new_value`.
pub(crate) fn futex_wake_or_set(address: &AtomicU32, count: i32, new_value: u32) -> i32 {
	if count < 0 {
		return -i32::from(Errno::Inval);
	}

	let address_usize = addr(address);
	let mut parking_lot = PARKING_LOT.lock_bucket(address_usize);
	let Some(mut queue) = parking_lot.get_pop_list(address_usize) else {
		address.store(new_value, SeqCst);
		return 0;
	};

	let scheduler = core_scheduler();
	let mut woken = 0;
	while woken != count || count == i32::MAX {
		match queue.pop() {
			Some(handle) => scheduler.custom_wakeup(handle),
			None => break,
		}
		woken = woken.saturating_add(1);
	}

	if woken == 0 {
		address.store(new_value, SeqCst);
	}

	woken
}
