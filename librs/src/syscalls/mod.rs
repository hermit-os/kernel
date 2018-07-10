// Copyright (c) 2018 Colin Finck, RWTH Aachen University
//                    Stefan Lankes, RWTH Aachen University
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

mod interfaces;
mod lwip;
mod processor;
mod random;
mod recmutex;
mod semaphore;
mod spinlock;
mod system;
mod tasks;
mod timer;

pub use self::lwip::*;
pub use self::processor::*;
pub use self::random::*;
pub use self::recmutex::*;
pub use self::semaphore::*;
pub use self::spinlock::*;
pub use self::system::*;
pub use self::tasks::*;
pub use self::timer::*;
use environment;
use synch::spinlock::SpinlockIrqSave;
use syscalls::interfaces::SyscallInterface;

const LWIP_FD_BIT: i32	= (1 << 30);

pub static LWIP_LOCK: SpinlockIrqSave<()> = SpinlockIrqSave::new(());
static mut SYS: &'static SyscallInterface = &interfaces::Generic;

pub fn enable_networking() {
	unsafe {
		// We know that HermitCore has successfully initialized a network interface.
		// Now check if we can load a more specific SyscallInterface to make use of networking.
		if environment::is_proxy() {
			SYS = &interfaces::Proxy;
		} else if environment::is_uhyve() {
			SYS = &interfaces::Uhyve;
		}

		// Perform interface-specific initialization steps.
		SYS.init();
	}
}

pub fn get_application_parameters() -> (i32, *mut *mut u8, *mut *mut u8) {
	unsafe { SYS.get_application_parameters() }
}

#[no_mangle]
pub extern "C" fn sys_shutdown() -> ! {
	unsafe { SYS.shutdown() }
}

#[no_mangle]
pub extern "C" fn sys_open(name: *const u8, flags: i32, mode: i32) -> i32 {
	unsafe { SYS.open(name, flags, mode) }
}

#[no_mangle]
pub extern "C" fn sys_close(fd: i32) -> i32 {
	unsafe { SYS.close(fd) }
}

#[no_mangle]
pub extern "C" fn sys_read(fd: i32, buf: *mut u8, len: usize) -> isize {
	unsafe { SYS.read(fd, buf, len) }
}

#[no_mangle]
pub extern "C" fn sys_write(fd: i32, buf: *const u8, len: usize) -> isize {
	unsafe { SYS.write(fd, buf, len) }
}

#[no_mangle]
pub extern "C" fn sys_lseek(fd: i32, offset: isize, whence: i32) -> isize {
	unsafe { SYS.lseek(fd, offset, whence) }
}

#[no_mangle]
pub extern "C" fn sys_stat(file: *const u8, st: usize) -> i32 {
	unsafe { SYS.stat(file, st) }
}

#[no_mangle]
pub extern "C" fn sys_putchar(character: u8) {
	unsafe { SYS.putchar(character); }
}
