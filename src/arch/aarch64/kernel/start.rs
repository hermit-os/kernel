// Copyright (c) 2020 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use crate::arch::aarch64::kernel::serial::SerialPort;
use crate::KERNEL_STACK_SIZE;

static mut BOOT_STACK: [u8; KERNEL_STACK_SIZE] = [0; KERNEL_STACK_SIZE];

#[inline(never)]
#[no_mangle]
#[naked]
pub unsafe extern "C" fn _start() -> ! {
    // initialize stack pointer
    llvm_asm!("ldr x1, =$0; mov sp, x1" :: "r"(&BOOT_STACK[0]+KERNEL_STACK_SIZE-0x10) :: "volatile");

    //pre_init();
    loop {}
}

unsafe fn pre_init() -> ! {
    let com1 = SerialPort::new(0x9000000);

    com1.write_byte('H' as u8);
    loop {}
}