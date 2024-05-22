// The implementation is inspired by Andrew D. Birrell's paper
// "Implementing Condition Variables with Semaphores"

use alloc::boxed::Box;
use core::sync::atomic::{AtomicIsize, Ordering};
use core::{mem, ptr};

use crate::synch::semaphore::Semaphore;

struct CondQueue {
	counter: AtomicIsize,
	sem1: Semaphore,
	sem2: Semaphore,
}

impl CondQueue {
	pub fn new() -> Self {
		CondQueue {
			counter: AtomicIsize::new(0),
			sem1: Semaphore::new(0),
			sem2: Semaphore::new(0),
		}
	}
}

#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_destroy_queue(ptr: usize) -> i32 {
	unsafe {
		let id = ptr::with_exposed_provenance_mut::<usize>(ptr);
		if id.is_null() {
			debug!("sys_wait: invalid address to condition variable");
			return -1;
		}

		if *id != 0 {
			let cond = Box::from_raw(ptr::with_exposed_provenance_mut::<CondQueue>(*id));
			mem::drop(cond);
		}

		0
	}
}

#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_notify(ptr: usize, count: i32) -> i32 {
	unsafe {
		let id = ptr::with_exposed_provenance::<usize>(ptr);

		if id.is_null() {
			// invalid argument
			debug!("sys_notify: invalid address to condition variable");
			return -1;
		}

		if *id == 0 {
			debug!("sys_notify: invalid reference to condition variable");
			return -1;
		}

		let cond = &mut *(ptr::with_exposed_provenance_mut::<CondQueue>(*id));

		if count < 0 {
			// Wake up all task that has been waiting for this condition variable
			while cond.counter.load(Ordering::SeqCst) > 0 {
				cond.counter.fetch_sub(1, Ordering::SeqCst);
				cond.sem1.release();
				cond.sem2.acquire(None);
			}
		} else {
			for _ in 0..count {
				cond.counter.fetch_sub(1, Ordering::SeqCst);
				cond.sem1.release();
				cond.sem2.acquire(None);
			}
		}

		0
	}
}

#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_init_queue(ptr: usize) -> i32 {
	unsafe {
		let id = ptr::with_exposed_provenance_mut::<usize>(ptr);
		if id.is_null() {
			debug!("sys_init_queue: invalid address to condition variable");
			return -1;
		}

		if *id == 0 {
			debug!("Create condition variable queue");
			let queue = Box::new(CondQueue::new());
			*id = Box::into_raw(queue) as usize;
		}

		0
	}
}

#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_add_queue(ptr: usize, timeout_ns: i64) -> i32 {
	unsafe {
		let id = ptr::with_exposed_provenance_mut::<usize>(ptr);
		if id.is_null() {
			debug!("sys_add_queue: invalid address to condition variable");
			return -1;
		}

		if *id == 0 {
			debug!("Create condition variable queue");
			let queue = Box::new(CondQueue::new());
			*id = Box::into_raw(queue) as usize;
		}

		if timeout_ns <= 0 {
			let cond = &mut *(ptr::with_exposed_provenance_mut::<CondQueue>(*id));
			cond.counter.fetch_add(1, Ordering::SeqCst);

			0
		} else {
			error!("Conditional variables with timeout is currently not supported");

			-1
		}
	}
}

#[hermit_macro::system]
#[no_mangle]
pub unsafe extern "C" fn sys_wait(ptr: usize) -> i32 {
	unsafe {
		let id = ptr::with_exposed_provenance_mut::<usize>(ptr);
		if id.is_null() {
			debug!("sys_wait: invalid address to condition variable");
			return -1;
		}

		if *id == 0 {
			error!("sys_wait: Unable to determine condition variable");
			return -1;
		}

		let cond = &mut *(ptr::with_exposed_provenance_mut::<CondQueue>(*id));
		cond.sem1.acquire(None);
		cond.sem2.release();

		0
	}
}
