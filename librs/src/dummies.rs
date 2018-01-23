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
use syscalls::tid_t;


#[link_section = ".percore"]
#[no_mangle]
pub static mut current_task_lwip_errno: i32 = 0;

#[no_mangle]
pub static mut rcce_lock: usize = 0;


#[no_mangle]
pub extern "C" fn block_current_task() -> i32 {
	panic!("block_current_task is unimplemented");
}

#[no_mangle]
pub extern "C" fn do_exit(arg: i32) -> ! {
	panic!("do_exit is unimplemented");
}

#[no_mangle]
pub extern "C" fn kputchar(character: i32) -> i32 {
	arch::output_message_byte(character as u8);
	1
}

#[no_mangle]
pub extern "C" fn reschedule() {
	panic!("reschedule is unimplemented");
}

#[no_mangle]
pub extern "C" fn wakeup_task(id: tid_t) -> i32 {
	panic!("wakeup_task is unimplemented");
}
