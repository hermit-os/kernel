// Copyright (c) 2019 Stefan Lankes, RWTH Aachen University
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use arch::x86_64::kernel::KERNEL_HEADER;
use x86::msr::*;
use boot_processor_main;
use application_processor_main;
use core::{mem,slice,ptr};
use config::*;

unsafe fn run_init_array(
	init_array_start: &extern "C" fn(),
	init_array_end: &extern "C" fn(),
) {
	let n = (init_array_end as *const _ as usize -
		init_array_start as *const _ as usize) /
		mem::size_of::<extern "C" fn()>();

	for f in slice::from_raw_parts(init_array_start, n) {
		f();
	}
}

#[cfg(not(test))]
#[inline(never)]
#[no_mangle]
#[naked]
pub unsafe extern "C" fn _start() -> ! {
	extern "C" {
		#[linkage = "extern_weak"]
		static __preinit_array_start: *const u8;
		#[linkage = "extern_weak"]
		static __preinit_array_end: *const u8;
		#[linkage = "extern_weak"]
		static __init_array_start: *const u8;
		#[linkage = "extern_weak"]
		static __init_array_end: *const u8;
		static tls_start: u8;
		static tls_end: u8;

		fn _init();
	}

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
		:: "r"(ptr::read_volatile(&KERNEL_HEADER.current_stack_address) + KERNEL_STACK_SIZE as u64 - 0x10)
		:: "volatile");

	// enable FPU & Cache
	asm!("mov %cr0, %rax
          or  $$0x22, %rax
	      mov $$0x6000000C, %rdx
	      not %rdx
	      and %rdx, %rax
          mov %rax, %cr0" :::: "volatile");

	// enable SSE (available on all x86_64 processors)
	asm!("mov %cr4, %rax
	      or  $$0x620, %rax
	      mov %rax, %cr4" :::: "volatile");

	let stack_start = ptr::read_volatile(&KERNEL_HEADER.current_stack_address) as usize;

	// set thread local storage
	let tls_size =  &tls_end as *const _ as usize - &tls_start as *const _ as usize;
	ptr::write_bytes(stack_start as *mut u8, 0, tls_size + 0x20usize);

	// The tls_pointer is the address to the end of the TLS area requested by the task.
	let tls_pointer = ((stack_start + 0x20usize) & !0x1Fusize) + tls_size;

	// The x86-64 TLS specification also requires that the tls_pointer can be accessed at fs:0.
	// This allows TLS variable values to be accessed by "mov rax, fs:0" and a later "lea rdx, [rax+VARIABLE_OFFSET]".
	// See "ELF Handling For Thread-Local Storage", version 0.20 by Ulrich Drepper, page 12 for details.
	//
	// fs:0 is where tls_pointer points to and we have reserved space for a usize value above.
	*(tls_pointer as *mut usize) = tls_pointer;

	// Copy over TLS variables with their initial values.
	ptr::copy_nonoverlapping(&tls_start as *const u8, (tls_pointer - tls_size) as *mut u8, tls_size);

	wrmsr(IA32_FS_BASE, tls_pointer as u64);

	// run preinit array
	if __preinit_array_end as usize - __preinit_array_start as usize > 0 {
		run_init_array(mem::transmute::<&*const u8, &extern "C" fn()>(&__preinit_array_start), mem::transmute::<&*const u8, &extern "C" fn()>(&__preinit_array_end));
	}

	_init();

	// run init array
	if __init_array_end as usize - __init_array_start as usize > 0 {
		run_init_array(mem::transmute::<&*const u8, &extern "C" fn()>(&__init_array_start), mem::transmute::<&*const u8, &extern "C" fn()>(&__init_array_end));
	}

	if ptr::read_volatile(&KERNEL_HEADER.cpu_online) == 0 {
		boot_processor_main();
	} else {
		application_processor_main();
	}
}
