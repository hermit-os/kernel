// Copyright (c) 2019 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use application_processor_main;
use arch::x86_64::kernel::{BootInfo, BOOT_INFO};
use boot_processor_main;
use config::KERNEL_STACK_SIZE;
use x86::controlregs::*;

pub unsafe fn cr0_enable_caching() {
	let mut cr0 = cr0();

	// Enable caching.
	cr0.remove(Cr0::CR0_CACHE_DISABLE | Cr0::CR0_NOT_WRITE_THROUGH);

	cr0_write(cr0);
}

#[inline(never)]
#[no_mangle]
#[naked]
pub unsafe extern "C" fn _start(boot_info: &'static mut BootInfo) -> ! {
	// initialize stack pointer
	llvm_asm!("mov $0, %rsp; mov %rsp, %rbp"
		:: "r"(boot_info.current_stack_address + KERNEL_STACK_SIZE as u64 - 0x10)
		:: "volatile");

	BOOT_INFO = boot_info as *mut BootInfo;

	//
	// CR0 CONFIGURATION
	//
	cr0_enable_caching();

	if boot_info.cpu_online == 0 {
		boot_processor_main();
	} else {
		application_processor_main();
	}
}
