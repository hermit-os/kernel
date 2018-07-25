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

pub fn set_oneshot_timer(wakeup_time: Option<u64>) {
	// TODO
	debug!("set_oneshot_timer stub");
}

pub fn wakeup_core(core_to_wakeup: usize) {
	// TODO
	debug!("wakeup_core stub");
}

#[no_mangle]
pub extern "C" fn do_bad_mode() {
}

#[no_mangle]
pub extern "C" fn do_error() {
}

#[no_mangle]
pub extern "C" fn do_fiq() {
}

#[no_mangle]
pub extern "C" fn do_irq() {
}

#[no_mangle]
pub extern "C" fn do_sync() {
}

#[no_mangle]
pub extern "C" fn eoi() {
}

#[no_mangle]
pub extern "C" fn finish_task_switch() {
}

#[no_mangle]
pub extern "C" fn get_current_stack() {
}

#[no_mangle]
pub extern "C" fn switch() {
}
