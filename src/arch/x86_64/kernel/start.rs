// Copyright (c) 2019 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use application_processor_main;
use arch::x86_64::kernel::KERNEL_HEADER;
use boot_processor_main;
use config::KERNEL_STACK_SIZE;
use core::intrinsics;

#[inline(never)]
#[no_mangle]
#[naked]
pub unsafe extern "C" fn _start() -> ! {
	// reset registers to kill any stale realmode selectors
	asm!("mov $$0x10, %rax
		mov %eax, %ds
		mov %eax, %ss
		mov %eax, %es
		xor %eax, %eax
		mov %eax, %fs
		mov %eax, %gs
		cld" :::: "volatile");

	// initialize stack pointer
	asm!("mov $0, %rsp; mov %rsp, %rbp"
		:: "r"(intrinsics::volatile_load(&KERNEL_HEADER.current_stack_address) + KERNEL_STACK_SIZE as u64 - 0x10)
		:: "volatile");

	if intrinsics::volatile_load(&KERNEL_HEADER.cpu_online) == 0 {
		boot_processor_main();
	} else {
		application_processor_main();
	}
}
