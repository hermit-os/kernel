/* Copyright (c) 2019 Stefan Lankes, RWTH Aachen University
 *
 * MIT License
 *
 * Permission is hereby granted, free of charge, to any person obtaining
 * a copy of this software and associated documentation files (the
 * "Software"), to deal in the Software without restriction, including
 * without limitation the rights to use, copy, modify, merge, publish,
 * distribute, sublicense, and/or sell copies of the Software, and to
 * permit persons to whom the Software is furnished to do so, subject to
 * the following conditions:
 *
 * The above copyright notice and this permission notice shall be
 * included in all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
 * EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
 * MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
 * NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
 * LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
 * OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
 * WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
 */

use arch::x86_64::kernel::KERNEL_HEADER;
use arch::x86_64::kernel::percore::PERCORE;
use boot_processor_main;
use application_processor_main;
use core::ptr;
use config::*;

#[cfg(not(test))]
#[inline(never)]
#[no_mangle]
#[naked]
pub unsafe extern "C" fn _start() {
	// reset registers to kill any stale realmode selectors
	asm!("mov $$0x10, %rax\n\t\
		mov %eax, %ds
		mov %eax, %ss
		mov %eax, %es
		xor %eax, %eax
		mov %eax, %fs
		mov %eax, %gs
		cld" :::: "volatile");

	// initialize stack pointer
	asm!("mov $0, %rsp; mov %rsp, %rbp"
		:: "r"(ptr::read_volatile(&KERNEL_HEADER.current_stack_address) + KERNEL_STACK_SIZE as u64 - 0x10)
		:: "volatile");

	if ptr::read_volatile(&KERNEL_HEADER.cpu_online) == 0	 {
		ptr::write_volatile(&mut KERNEL_HEADER.current_percore_address, &PERCORE as *const _ as u64);
		boot_processor_main();
	} else {
		application_processor_main();
	}
}
