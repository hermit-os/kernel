// Copyright (c) 2018 Colin Finck, RWTH Aachen University
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

use arch;
use arch::percore::*;
use alloc::rc::Rc;
use core::isize;
use core::cell::RefCell;
use errno::*;
use scheduler;
use scheduler::task::{Priority,TaskId};

pub type signal_handler_t = extern "C" fn(i32);
pub type tid_t = u32;


#[no_mangle]
pub extern "C" fn sys_getpid() -> tid_t {
	let current_task_borrowed = core_scheduler().current_task.borrow();
	current_task_borrowed.id.into() as tid_t
}

#[no_mangle]
pub extern "C" fn sys_getprio(id: *const tid_t) -> i32 {
	let current_task_borrowed = core_scheduler().current_task.borrow();

	if id.is_null() || unsafe {*id} == current_task_borrowed.id.into() as u32 {
		current_task_borrowed.prio.into() as i32
	} else {
		-EINVAL
	}
}

#[no_mangle]
pub extern "C" fn sys_setprio(_id: *const tid_t, _prio: i32) -> i32 {
	-ENOSYS
}

#[no_mangle]
pub extern "C" fn sys_exit(arg: i32) -> ! {
	core_scheduler().exit(arg);
}

#[no_mangle]
pub extern "C" fn sys_sbrk(incr: isize) -> usize {
	// Get the boundaries of the task heap and verify that they are suitable for sbrk.
	let task_heap_start = arch::mm::virtualmem::task_heap_start();
	let task_heap_end = arch::mm::virtualmem::task_heap_end();
	assert!(task_heap_end <= isize::MAX as usize);

	// Get the heap of the current task on the current core.
	let mut current_task_borrowed = core_scheduler().current_task.borrow_mut();
	let heap = current_task_borrowed.heap.as_mut().expect("Calling sys_sbrk on a task without an associated heap");

	// Adjust the heap of the current task.
	let heap_borrowed = heap.borrow();
	let mut heap_locked = heap_borrowed.write();
	assert!(heap_locked.start >= task_heap_start, "heap start {:#X} is not >= task_heap_start {:#X}", heap_locked.start, task_heap_start);
	let old_end = heap_locked.end;
	heap_locked.end = (old_end as isize + incr) as usize;
	assert!(heap_locked.end <= task_heap_end, "New heap end {:#X} is not <= task_heap_end {:#X}", heap_locked.end, task_heap_end);

	debug!("Adjusted task heap from {:#X} to {:#X}", old_end, heap_locked.end);

	// We're done! The page fault handler will map the new virtual memory area to physical memory
	// as soon as the task accesses it for the first time.
	old_end
}

// TODO: Rename this function to sys_usleep for consistency and change the call in GCC's libgo/runtime/yield.c
// This is a breaking change though!
// Not doing this yet allows us to use the same GCC for the HermitCore C version and HermitCore-rs.
#[no_mangle]
pub extern "C" fn udelay(usecs: u32) {
	let ticks = (usecs as usize / 1000) * arch::processor::TIMER_FREQUENCY / 1000;

	if ticks > 0 {
		// Enough time to set a wakeup timer and block the current task.
		debug!("udelay waiting {} ticks", ticks);
		let wakeup_time = arch::processor::update_timer_ticks() + ticks;
		let core_scheduler = core_scheduler();
		let current_task = core_scheduler.current_task.clone();
		core_scheduler.blocked_tasks.lock().add(current_task, Some(wakeup_time));

		// Switch to the next task.
		core_scheduler.scheduler();
	} else if usecs > 0 {
		// Not enough time to set a wakeup timer, so just do busy-waiting.
		arch::processor::udelay(usecs as u64);
	}
}

#[no_mangle]
pub extern "C" fn sys_msleep(ms: u32) {
	udelay(ms * 1000);
}

#[no_mangle]
pub extern "C" fn sys_clone(id: *mut tid_t, func: extern "C" fn(usize), arg: usize) -> i32 {
	let task_id = core_scheduler().clone(func, arg);

	if !id.is_null() {
		unsafe { *id = task_id.into() as u32; }
	}

	0
}

#[no_mangle]
pub extern "C" fn sys_yield() {
	core_scheduler().scheduler();
}

#[no_mangle]
pub extern "C" fn sys_kill(dest: tid_t, signum: i32) -> i32 {
	debug!("sys_kill is unimplemented, returning -ENOSYS for killing {} with signal {}", dest, signum);
	-ENOSYS
}

#[no_mangle]
pub extern "C" fn sys_signal(handler: signal_handler_t) -> i32 {
	debug!("sys_signal is unimplemented");
	0
}

#[no_mangle]
pub extern "C" fn sys_spawn(id: *mut tid_t, func: extern "C" fn(usize), arg: usize, prio: u8, core_id: u32) -> i32 {
	let core_scheduler = scheduler::get_scheduler(core_id);
	let task_id = core_scheduler.spawn(func, arg, Priority::from(prio), None);

	if !id.is_null() {
		unsafe { *id = task_id.into() as u32; }
	}

	0
}

#[no_mangle]
pub extern "C" fn reschedule() {
	// Switch to the next task.
	core_scheduler().scheduler();
}

// just to be backward compatible to the C version
#[no_mangle]
pub extern "C" fn do_exit(arg: i32) -> ! {
	core_scheduler().exit(arg);
}

#[no_mangle]
pub extern "C" fn block_current_task() {
	let core_scheduler = core_scheduler();

	// Block the current task and add it to the wakeup queue.
	core_scheduler.blocked_tasks.lock().add(core_scheduler.current_task.clone(), None);
}

#[no_mangle]
pub extern "C" fn wakeup_task(id: tid_t) {
	core_scheduler().blocked_tasks.lock().wakeup_by_id(TaskId::from(id as usize));
}
