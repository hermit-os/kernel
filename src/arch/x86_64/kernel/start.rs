// Copyright (c) 2019 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

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
