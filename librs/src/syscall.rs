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

use errno::*;

pub type signal_handler_t = extern "C" fn(i32);
pub type tid_t = u32;


/// Called by libpthread.
#[no_mangle]
pub extern "C" fn do_exit(arg: i32) -> ! {
	panic!("do_exit is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_getpid() -> tid_t {
	panic!("sys_getpid is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_getprio(id: *const tid_t) -> i32 {
	panic!("sys_getprio is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_setprio(id: *const tid_t, prio: i32) -> i32 {
	-ENOSYS
}

#[no_mangle]
pub extern "C" fn sys_exit(arg: i32) -> ! {
	panic!("sys_exit is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_read(fd: i32, buf: *mut u8, len: usize) -> isize {
	panic!("sys_read is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_write(fd: i32, buf: *const u8, len: usize) -> isize {
	panic!("sys_write is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_sbrk(incr: isize) -> isize {
	panic!("sys_sbrk is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_open(name: *const u8, flags: i32, mode: i32) -> i32 {
	panic!("sys_open is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_close(fd: i32) -> i32 {
	panic!("sys_close is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_msleep(ms: u32) {
	panic!("sys_msleep is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_sem_init(sem: usize, value: u32) -> i32 {
	panic!("sys_sem_init is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_sem_destroy(sem: usize) -> i32 {
	panic!("sys_sem_destroy is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_sem_wait(sem: usize) -> i32 {
	panic!("sys_sem_wait is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_sem_post(sem: usize) -> i32 {
	panic!("sys_sem_post is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_sem_timedwait(sem: usize, ms: u32) -> i32 {
	panic!("sys_sem_timedwait is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_sem_cancelablewait(sem: usize, ms: u32) -> i32 {
	panic!("sys_sem_cancelablewait is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_clone(id: *const tid_t, ep: usize, argv: usize) -> i32 {
	panic!("sys_clone is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_lseek(fd: i32, offset: isize, whence: i32) -> isize {
	panic!("sys_lseek is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_rcce_init(session_id: i32) -> i32 {
	panic!("sys_rcce_init is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_rcce_malloc(session_id: i32, ue: i32) -> usize {
	panic!("sys_rcce_malloc is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_rcce_fini(session_id: i32) -> i32 {
	panic!("sys_rcce_fini is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_get_ticks() -> usize {
	panic!("sys_get_ticks is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_stat(file: *const u8, st: usize) -> i32 {
	-ENOSYS
}

#[no_mangle]
pub extern "C" fn sys_yield() {
	panic!("sys_yield is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_kill(dest: tid_t, signum: i32) -> i32 {
	panic!("sys_kill is unimplemented");
}

#[no_mangle]
pub extern "C" fn sys_signal(handler: signal_handler_t) -> i32 {
	panic!("sys_signal is unimplemented");
}
